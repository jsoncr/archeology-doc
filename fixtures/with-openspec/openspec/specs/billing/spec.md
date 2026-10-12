# billing Specification

## Purpose

Gestión de facturación y descuentos.

## Requirements

### Requirement: Add numbers

The system SHALL add two numbers.

#### Scenario: Basic addition

- **WHEN** adding 1 and 2
- **THEN** the result is 3

### Requirement: Apply discount

The system SHALL apply a discount with a maximum of 10 percent.

#### Scenario: Discount applied

- **WHEN** applying a discount under the limit
- **THEN** the price is reduced accordingly

### Requirement: Round totals

The system SHALL round invoice totals to two decimals using half-up rounding.

#### Scenario: Half-up rounding

- **WHEN** the computed total is 10.005
- **THEN** the invoice total is 10.01
