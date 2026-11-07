-- HyperStack bench: ClickHouse `shop` scenario tables - products with one
-- negative and one zero stock, 48 hourly events over two days. Re-runnable.
CREATE DATABASE IF NOT EXISTS shop;
DROP TABLE IF EXISTS shop.events;
DROP TABLE IF EXISTS shop.products;
CREATE TABLE shop.events (ts DateTime, user_id UInt32, kind LowCardinality(String), amount Decimal(10,2)) ENGINE = MergeTree ORDER BY (ts, user_id);
CREATE TABLE shop.products (id UInt32, name String, price Decimal(10,2), stock Int32) ENGINE = MergeTree ORDER BY id;
INSERT INTO shop.products VALUES (1,'Keyboard',49.90,120),(2,'Mouse',19.50,0),(3,'Monitor 27"',289.00,14),(4,'USB-C Hub',35.00,-2),(5,'Webcam',59.00,33);
INSERT INTO shop.events SELECT now() - number*3600, 1 + number % 7, ['view','cart','purchase','refund'][1 + number % 4], round(number * 3.75, 2) FROM numbers(48);
SELECT concat(database, '.', name, ' rows=', toString(total_rows)) FROM system.tables WHERE database = 'shop' FORMAT TSVRaw;
