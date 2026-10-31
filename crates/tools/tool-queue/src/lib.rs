//! HyperStack Queue tool (docs/tools/queue.md): the message brokers a team
//! runs - RabbitMQ, Kafka, Redis Streams, NATS and MQTT - behind one
//! [`driver::Driver`] trait, one tab per broker.

pub mod driver;
pub mod kafka;
pub mod kafka_groups;
pub mod mqtt;
pub mod mqtt_wire;
pub mod nats;
pub mod payload;
pub mod profile;
pub mod rabbitmq;
pub mod redis_streams;
pub mod runner;
mod tls;
pub mod ui;

pub use ui::{DESCRIPTOR, QueueFactory};
