-- HyperStack bench: the `shop` database for SQL Server, the same tables and
-- rows as the Postgres bench. Safe to re-run (drops and recreates the tables).
IF DB_ID('shop') IS NULL CREATE DATABASE shop;
GO
USE shop;
GO
DROP VIEW IF EXISTS dbo.order_totals;
DROP TABLE IF EXISTS dbo.order_lines;
DROP TABLE IF EXISTS dbo.orders;
DROP TABLE IF EXISTS dbo.products;
DROP TABLE IF EXISTS dbo.customers;
GO
CREATE TABLE dbo.customers (
  id INT IDENTITY(1,1) PRIMARY KEY,
  name NVARCHAR(100) NOT NULL,
  email NVARCHAR(200) NOT NULL UNIQUE,
  city NVARCHAR(100) NULL,
  joined DATE NOT NULL DEFAULT CAST(GETDATE() AS DATE)
);
CREATE TABLE dbo.products (
  id INT IDENTITY(1,1) PRIMARY KEY,
  sku NVARCHAR(20) NOT NULL UNIQUE,
  name NVARCHAR(100) NOT NULL,
  price DECIMAL(10,2) NOT NULL,
  in_stock INT NOT NULL DEFAULT 0
);
CREATE TABLE dbo.orders (
  id INT IDENTITY(1,1) PRIMARY KEY,
  customer_id INT NULL REFERENCES dbo.customers(id),
  placed DATETIMEOFFSET NOT NULL DEFAULT SYSDATETIMEOFFSET(),
  status NVARCHAR(20) NOT NULL DEFAULT 'new'
);
CREATE TABLE dbo.order_lines (
  id INT IDENTITY(1,1) PRIMARY KEY,
  order_id INT NULL REFERENCES dbo.orders(id),
  product_id INT NULL REFERENCES dbo.products(id),
  qty INT NOT NULL
);
GO
INSERT INTO dbo.customers (name, email, city, joined) VALUES
  ('Ada Lovelace', 'ada@example.com', 'London', '2026-10-24'),
  ('Alan Turing', 'alan@example.com', 'Manchester', '2026-10-24'),
  ('Grace Hopper', 'grace@example.com', 'New York', '2026-10-24'),
  ('Katherine Johnson', 'katherine@example.com', 'Hampton', '2026-10-24'),
  ('Linus Torvalds', 'linus@example.com', 'Portland', '2026-10-24');
INSERT INTO dbo.products (sku, name, price, in_stock) VALUES
  ('KEY-001', 'Mechanical Keyboard', 129.00, 42),
  ('MON-002', '27in Monitor', 349.50, 11),
  ('CAB-003', 'USB-C Cable', 12.99, 500),
  ('DSK-004', 'Standing Desk', 615.00, 3),
  ('HUB-005', 'Thunderbolt Hub', 199.00, 0);
INSERT INTO dbo.orders (customer_id, placed, status) VALUES
  (1, '2026-10-24T09:46:26+00:00', 'shipped'),
  (2, '2026-10-24T09:46:26+00:00', 'new'),
  (1, '2026-10-24T09:46:26+00:00', 'cancelled'),
  (3, '2026-10-24T09:46:26+00:00', 'shipped'),
  (5, '2026-10-24T09:46:26+00:00', 'new');
INSERT INTO dbo.order_lines (order_id, product_id, qty) VALUES
  (1, 1, 1), (1, 3, 2), (2, 2, 1), (3, 4, 1), (4, 5, 1), (4, 3, 3), (5, 1, 2);
GO
CREATE VIEW dbo.order_totals AS
  SELECT o.id, c.name AS customer, o.status, SUM(p.price * l.qty) AS total
  FROM dbo.orders o
  JOIN dbo.customers c ON c.id = o.customer_id
  JOIN dbo.order_lines l ON l.order_id = o.id
  JOIN dbo.products p ON p.id = l.product_id
  GROUP BY o.id, c.name, o.status;
GO
SET NOCOUNT ON;
SELECT 'customers=' + CAST(COUNT(*) AS VARCHAR) FROM dbo.customers;
SELECT 'orders=' + CAST(COUNT(*) AS VARCHAR) FROM dbo.orders;
GO
