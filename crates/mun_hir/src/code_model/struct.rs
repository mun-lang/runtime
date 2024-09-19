use std::{fmt, iter::once, sync::Arc};

pub use ast::StructMemoryKind;
use mun_syntax::{
    ast,
    ast::{NameOwner, TypeAscriptionOwner, VisibilityOwner},
};

use super::{
    field::{Field, FieldsData},
    Module,
};
use crate::{
    has_module::HasModule,
    ids::{Lookup, StructId},
    name::AsName,
    name_resolution::Namespace,
    resolve::HasResolver,
    ty::lower::LowerTyMap,
    type_ref::{LocalTypeRefId, TypeRefMap, TypeRefSourceMap},
    visibility::RawVisibility,
    DefDatabase, DiagnosticSink, FileId, HasVisibility, HirDatabase, Name, Ty, Visibility,
};

pub(crate) mod validator;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Struct {
    id: StructId,
}

impl Struct {
    pub fn module(self, db: &dyn HirDatabase) -> Module {
        self.id.module(db.upcast()).into()
    }

    pub fn file_id(self, db: &dyn HirDatabase) -> FileId {
        self.id.lookup(db.upcast()).id.file_id
    }

    pub fn data(self, db: &dyn DefDatabase) -> Arc<StructData> {
        db.struct_data(self.id)
    }

    /// Returns the name of the struct non including any module specifiers (e.g:
    /// `Bar`).
    pub fn name(self, db: &dyn HirDatabase) -> Name {
        self.data(db.upcast()).name.clone()
    }

    /// Returns the full name of the struct including all module specifiers
    /// (e.g: `foo::Bar`).
    pub fn full_name(self, db: &dyn HirDatabase) -> String {
        itertools::Itertools::intersperse(
            self.module(db)
                .path_to_root(db)
                .into_iter()
                .filter_map(|module| module.name(db))
                .chain(once(self.name(db)))
                .map(|name| name.to_string()),
            String::from("::"),
        )
        .collect()
    }

    pub fn fields(self, db: &dyn HirDatabase) -> Box<[Field]> {
        self.data(db.upcast())
            .fields_data
            .fields()
            .iter()
            .map(|(id, _)| Field {
                parent: self.into(),
                id,
            })
            .collect()
    }

    pub fn field(self, db: &dyn HirDatabase, name: &Name) -> Option<Field> {
        self.data(db.upcast())
            .fields_data
            .fields()
            .iter()
            .find(|(_, data)| data.name == *name)
            .map(|(id, _)| Field {
                parent: self.into(),
                id,
            })
    }

    pub fn ty(self, db: &dyn HirDatabase) -> Ty {
        db.type_for_def(self.into(), Namespace::Types)
    }

    pub fn lower(self, db: &dyn HirDatabase) -> Arc<LowerTyMap> {
        db.lower_struct(self)
    }

    pub fn diagnostics(self, db: &dyn HirDatabase, sink: &mut DiagnosticSink<'_>) {
        let data = self.data(db.upcast());
        let lower = self.lower(db);
        lower.add_diagnostics(db, self.file_id(db), data.type_ref_source_map(), sink);
        let validator = validator::StructValidator::new(self, db, self.file_id(db));
        validator.validate_privacy(sink);
    }
}

impl From<Struct> for StructId {
    fn from(value: Struct) -> Self {
        value.id
    }
}

impl From<StructId> for Struct {
    fn from(id: StructId) -> Self {
        Struct { id }
    }
}

/// A single field of a record
/// ```mun
/// struct Foo {
///     a: int, // <- this
/// }
/// ```
/// or
/// ```mun
/// struct Foo(
///     int, // <- this
/// )
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldData {
    pub name: Name,
    pub type_ref: LocalTypeRefId,
    pub visibility: RawVisibility,
}

/// A struct's fields' data (record, tuple, or unit struct)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructKind {
    Record,
    Tuple,
    Unit,
}

impl fmt::Display for StructKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StructKind::Record => write!(f, "record"),
            StructKind::Tuple => write!(f, "tuple"),
            StructKind::Unit => write!(f, "unit struct"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct StructData {
    pub name: Name,
    pub visibility: RawVisibility,
    pub fields_data: Arc<FieldsData>,
    pub memory_kind: StructMemoryKind,
    type_ref_map: TypeRefMap,
    type_ref_source_map: TypeRefSourceMap,
}

impl StructData {
    pub(crate) fn struct_data_query(db: &dyn DefDatabase, id: StructId) -> Arc<StructData> {
        let loc = id.lookup(db);
        let item_tree = db.item_tree(loc.id.file_id);
        let strukt = &item_tree[loc.id.value];
        let src = item_tree.source(db, loc.id.value);

        let memory_kind = src
            .memory_type_specifier()
            .map(|s| s.kind())
            .unwrap_or_default();

        let mut type_ref_builder = TypeRefMap::builder();
        let fields_data = match src.kind() {
            ast::StructKind::Record(r) => {
                let fields = r
                    .fields()
                    .map(|fd| FieldData {
                        name: fd.name().map_or_else(Name::missing, |n| n.as_name()),
                        type_ref: type_ref_builder.alloc_from_node_opt(fd.ascribed_type().as_ref()),
                        visibility: RawVisibility::from_ast(fd.visibility()),
                    })
                    .collect();
                FieldsData::Record(fields)
            }
            ast::StructKind::Tuple(t) => {
                let fields = t
                    .fields()
                    .enumerate()
                    .map(|(index, fd)| FieldData {
                        name: Name::new_tuple_field(index),
                        type_ref: type_ref_builder.alloc_from_node_opt(fd.type_ref().as_ref()),
                        visibility: RawVisibility::from_ast(fd.visibility()),
                    })
                    .collect();
                FieldsData::Tuple(fields)
            }
            ast::StructKind::Unit => FieldsData::Unit,
        };

        let visibility = item_tree[strukt.visibility].clone();

        let (type_ref_map, type_ref_source_map) = type_ref_builder.finish();
        Arc::new(StructData {
            name: strukt.name.clone(),
            visibility,
            fields_data: Arc::new(fields_data),
            memory_kind,
            type_ref_map,
            type_ref_source_map,
        })
    }

    pub fn type_ref_source_map(&self) -> &TypeRefSourceMap {
        &self.type_ref_source_map
    }

    pub fn type_ref_map(&self) -> &TypeRefMap {
        &self.type_ref_map
    }
}

impl HasVisibility for Struct {
    fn visibility(&self, db: &dyn HirDatabase) -> Visibility {
        self.data(db.upcast())
            .visibility
            .resolve(db.upcast(), &self.id.resolver(db.upcast()))
    }
}
