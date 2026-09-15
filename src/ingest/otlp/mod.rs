mod convert;
mod normalize;
mod receiver;
mod types;

pub use receiver::serve;
pub use types::{OtlpLogRecord, OtlpSpanRecord};
