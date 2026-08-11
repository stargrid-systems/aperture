//! Type-erased view of an [`EventDefinition`].
//!
//! The registry stores definitions behind this trait so it can hold many event
//! kinds together. A blanket impl bridges every [`EventDefinition`].

use utoipa::openapi::schema::Schema;
use utoipa::openapi::RefOr;
use utoipa::{PartialSchema, ToSchema};

use crate::definition::EventDefinition;

pub trait ErasedEventDefinition: Send + Sync + 'static {
    fn key(&self) -> &'static str;
    fn payload_name(&self) -> String;
    fn payload_schema(&self) -> RefOr<Schema>;
    fn collect_schemas(&self, out: &mut Vec<(String, RefOr<Schema>)>);
}

impl<T: EventDefinition> ErasedEventDefinition for T {
    fn key(&self) -> &'static str {
        T::KEY
    }

    fn payload_name(&self) -> String {
        <T as ToSchema>::name().into_owned()
    }

    fn payload_schema(&self) -> RefOr<Schema> {
        <T as PartialSchema>::schema()
    }

    fn collect_schemas(&self, out: &mut Vec<(String, RefOr<Schema>)>) {
        out.push((
            <T as ToSchema>::name().into_owned(),
            <T as PartialSchema>::schema(),
        ));
        <T as ToSchema>::schemas(out);
    }
}
