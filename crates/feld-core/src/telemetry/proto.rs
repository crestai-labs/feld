//! The OTLP/HTTP protobuf messages the Collector receives and the daemon decodes, with the
//! parts Feld reads, and the builders that write them: the status gauges a Plugin helper
//! sends, and the exports the tests stand in for a Runtime with.

use prost::Message;

#[derive(Clone, PartialEq, Message)]
pub struct ExportMetricsServiceRequest {
    #[prost(message, repeated, tag = "1")]
    pub resource_metrics: Vec<ResourceMetrics>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ExportLogsServiceRequest {
    #[prost(message, repeated, tag = "1")]
    pub resource_logs: Vec<ResourceLogs>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ResourceMetrics {
    #[prost(message, optional, tag = "1")]
    pub resource: Option<Resource>,
    #[prost(message, repeated, tag = "2")]
    pub scope_metrics: Vec<ScopeMetrics>,
    #[prost(string, tag = "3")]
    pub schema_url: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ResourceLogs {
    #[prost(message, optional, tag = "1")]
    pub resource: Option<Resource>,
    #[prost(message, repeated, tag = "2")]
    pub scope_logs: Vec<ScopeLogs>,
    #[prost(string, tag = "3")]
    pub schema_url: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ScopeLogs {
    #[prost(message, optional, tag = "1")]
    pub scope: Option<InstrumentationScope>,
    #[prost(message, repeated, tag = "2")]
    pub log_records: Vec<LogRecord>,
    #[prost(string, tag = "3")]
    pub schema_url: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct LogRecord {
    #[prost(fixed64, tag = "1")]
    pub time_unix_nano: u64,
    #[prost(message, optional, tag = "5")]
    pub body: Option<AnyValue>,
    #[prost(message, repeated, tag = "6")]
    pub attributes: Vec<KeyValue>,
    #[prost(fixed64, tag = "11")]
    pub observed_time_unix_nano: u64,
    #[prost(string, tag = "12")]
    pub event_name: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Resource {
    #[prost(message, repeated, tag = "1")]
    pub attributes: Vec<KeyValue>,
    #[prost(uint32, tag = "2")]
    pub dropped_attributes_count: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct ScopeMetrics {
    #[prost(message, optional, tag = "1")]
    pub scope: Option<InstrumentationScope>,
    #[prost(message, repeated, tag = "2")]
    pub metrics: Vec<Metric>,
    #[prost(string, tag = "3")]
    pub schema_url: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct InstrumentationScope {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub version: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Metric {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub description: String,
    #[prost(string, tag = "3")]
    pub unit: String,
    #[prost(oneof = "metric::Data", tags = "5, 7, 9, 10, 11")]
    pub data: Option<metric::Data>,
}

pub mod metric {
    use prost::Oneof;

    use super::{Gauge, Histogram, OpaqueMetric, Sum};

    #[derive(Clone, PartialEq, Oneof)]
    pub enum Data {
        #[prost(message, tag = "5")]
        Gauge(Gauge),
        #[prost(message, tag = "7")]
        Sum(Sum),
        #[prost(message, tag = "9")]
        Histogram(Histogram),
        #[prost(message, tag = "10")]
        ExponentialHistogram(OpaqueMetric),
        #[prost(message, tag = "11")]
        Summary(OpaqueMetric),
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct OpaqueMetric {}

#[derive(Clone, PartialEq, Message)]
pub struct Sum {
    #[prost(message, repeated, tag = "1")]
    pub data_points: Vec<NumberDataPoint>,
    #[prost(enumeration = "AggregationTemporality", tag = "2")]
    pub aggregation_temporality: i32,
    #[prost(bool, tag = "3")]
    pub is_monotonic: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum AggregationTemporality {
    Unspecified = 0,
    Delta = 1,
    Cumulative = 2,
}

#[derive(Clone, PartialEq, Message)]
pub struct Histogram {
    #[prost(message, repeated, tag = "1")]
    pub data_points: Vec<HistogramDataPoint>,
    #[prost(enumeration = "AggregationTemporality", tag = "2")]
    pub aggregation_temporality: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct HistogramDataPoint {
    #[prost(message, repeated, tag = "9")]
    pub attributes: Vec<KeyValue>,
    #[prost(fixed64, tag = "2")]
    pub start_time_unix_nano: u64,
    #[prost(fixed64, tag = "3")]
    pub time_unix_nano: u64,
    #[prost(fixed64, tag = "4")]
    pub count: u64,
    #[prost(double, optional, tag = "5")]
    pub sum: Option<f64>,
    #[prost(fixed64, repeated, tag = "6")]
    pub bucket_counts: Vec<u64>,
    #[prost(double, repeated, tag = "7")]
    pub explicit_bounds: Vec<f64>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Gauge {
    #[prost(message, repeated, tag = "1")]
    pub data_points: Vec<NumberDataPoint>,
}

#[derive(Clone, PartialEq, Message)]
pub struct NumberDataPoint {
    #[prost(message, repeated, tag = "7")]
    pub attributes: Vec<KeyValue>,
    #[prost(fixed64, tag = "2")]
    pub start_time_unix_nano: u64,
    #[prost(fixed64, tag = "3")]
    pub time_unix_nano: u64,
    #[prost(oneof = "number_data_point::Value", tags = "4, 6")]
    pub value: Option<number_data_point::Value>,
    #[prost(uint32, tag = "8")]
    pub flags: u32,
}

pub mod number_data_point {
    use prost::Oneof;

    #[derive(Clone, PartialEq, Oneof)]
    pub enum Value {
        #[prost(double, tag = "4")]
        AsDouble(f64),
        #[prost(sfixed64, tag = "6")]
        AsInt(i64),
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct KeyValue {
    #[prost(string, tag = "1")]
    pub key: String,
    #[prost(message, optional, tag = "2")]
    pub value: Option<AnyValue>,
}

#[derive(Clone, PartialEq, Message)]
pub struct AnyValue {
    #[prost(oneof = "any_value::Value", tags = "1, 2, 3, 4, 7")]
    pub value: Option<any_value::Value>,
}

pub mod any_value {
    use prost::Oneof;

    #[derive(Clone, PartialEq, Oneof)]
    pub enum Value {
        #[prost(string, tag = "1")]
        String(String),
        #[prost(bool, tag = "2")]
        Bool(bool),
        #[prost(int64, tag = "3")]
        Int(i64),
        #[prost(double, tag = "4")]
        Double(f64),
        #[prost(bytes, tag = "7")]
        Bytes(Vec<u8>),
    }
}

impl ExportMetricsServiceRequest {
    /// One export of `metrics` under one resource and scope.
    pub fn new(
        resource: Option<Resource>,
        scope: Option<InstrumentationScope>,
        metrics: Vec<Metric>,
    ) -> Self {
        Self {
            resource_metrics: vec![ResourceMetrics {
                resource,
                scope_metrics: vec![ScopeMetrics {
                    scope,
                    metrics,
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }],
        }
    }
}

impl ExportLogsServiceRequest {
    /// One export of `records` under one resource and scope.
    pub fn new(
        resource: Option<Resource>,
        scope: Option<InstrumentationScope>,
        records: Vec<LogRecord>,
    ) -> Self {
        Self {
            resource_logs: vec![ResourceLogs {
                resource,
                scope_logs: vec![ScopeLogs {
                    scope,
                    log_records: records,
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }],
        }
    }
}

impl Resource {
    pub fn new(attributes: Vec<KeyValue>) -> Self {
        Self {
            attributes,
            dropped_attributes_count: 0,
        }
    }
}

impl InstrumentationScope {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

impl Metric {
    fn of(name: impl Into<String>, data: metric::Data) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            unit: String::new(),
            data: Some(data),
        }
    }

    pub fn gauge(name: impl Into<String>, data_points: Vec<NumberDataPoint>) -> Self {
        Self::of(name, metric::Data::Gauge(Gauge { data_points }))
    }

    /// A monotonic sum.
    pub fn sum(
        name: impl Into<String>,
        temporality: AggregationTemporality,
        data_points: Vec<NumberDataPoint>,
    ) -> Self {
        Self::of(
            name,
            metric::Data::Sum(Sum {
                data_points,
                aggregation_temporality: temporality.into(),
                is_monotonic: true,
            }),
        )
    }

    pub fn histogram(
        name: impl Into<String>,
        temporality: AggregationTemporality,
        data_points: Vec<HistogramDataPoint>,
    ) -> Self {
        Self::of(
            name,
            metric::Data::Histogram(Histogram {
                data_points,
                aggregation_temporality: temporality.into(),
            }),
        )
    }
}

impl NumberDataPoint {
    pub fn double(start_time_unix_nano: u64, time_unix_nano: u64, value: f64) -> Self {
        Self::of(
            start_time_unix_nano,
            time_unix_nano,
            number_data_point::Value::AsDouble(value),
        )
    }

    pub fn int(start_time_unix_nano: u64, time_unix_nano: u64, value: i64) -> Self {
        Self::of(
            start_time_unix_nano,
            time_unix_nano,
            number_data_point::Value::AsInt(value),
        )
    }

    fn of(start_time_unix_nano: u64, time_unix_nano: u64, value: number_data_point::Value) -> Self {
        Self {
            attributes: Vec::new(),
            start_time_unix_nano,
            time_unix_nano,
            value: Some(value),
            flags: 0,
        }
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<AnyValue>) -> Self {
        self.attributes.push(KeyValue::new(key, value));
        self
    }
}

impl HistogramDataPoint {
    /// The observations of one interval: their sum, and the count of each bucket the bounds
    /// cut, the last of which is open above the highest bound.
    pub fn new(
        start_time_unix_nano: u64,
        time_unix_nano: u64,
        sum: f64,
        bounds: &[f64],
        counts: &[u64],
    ) -> Self {
        Self {
            attributes: Vec::new(),
            start_time_unix_nano,
            time_unix_nano,
            count: counts.iter().sum(),
            sum: Some(sum),
            bucket_counts: counts.to_vec(),
            explicit_bounds: bounds.to_vec(),
        }
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<AnyValue>) -> Self {
        self.attributes.push(KeyValue::new(key, value));
        self
    }
}

impl LogRecord {
    /// A record observed when it was written.
    pub fn new(time_unix_nano: u64) -> Self {
        Self {
            time_unix_nano,
            body: None,
            attributes: Vec::new(),
            observed_time_unix_nano: time_unix_nano,
            event_name: String::new(),
        }
    }

    pub fn with_body(mut self, body: impl Into<AnyValue>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn with_event_name(mut self, event_name: impl Into<String>) -> Self {
        self.event_name = event_name.into();
        self
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<AnyValue>) -> Self {
        self.attributes.push(KeyValue::new(key, value));
        self
    }
}

impl KeyValue {
    pub fn new(key: impl Into<String>, value: impl Into<AnyValue>) -> Self {
        Self {
            key: key.into(),
            value: Some(value.into()),
        }
    }
}

impl From<any_value::Value> for AnyValue {
    fn from(value: any_value::Value) -> Self {
        Self { value: Some(value) }
    }
}

impl From<&str> for AnyValue {
    fn from(value: &str) -> Self {
        any_value::Value::String(value.to_owned()).into()
    }
}

impl From<String> for AnyValue {
    fn from(value: String) -> Self {
        any_value::Value::String(value).into()
    }
}

impl From<bool> for AnyValue {
    fn from(value: bool) -> Self {
        any_value::Value::Bool(value).into()
    }
}

impl From<i64> for AnyValue {
    fn from(value: i64) -> Self {
        any_value::Value::Int(value).into()
    }
}

impl From<f64> for AnyValue {
    fn from(value: f64) -> Self {
        any_value::Value::Double(value).into()
    }
}
