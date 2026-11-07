-- Run before db2-shop.sql; on a fresh database every statement here fails
-- (nothing to drop yet), which seed-db2.sh expects and ignores.
DROP VIEW order_totals;
DROP TABLE order_lines;
DROP TABLE orders;
DROP TABLE products;
DROP TABLE customers;
