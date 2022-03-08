# Payment Engine in Rust
This program takes a .csv file, processes the transactions contained in the file, and outputs an account ledger to stdout in CSV format.

## Run command
```
$ cargo run -- transactions.csv > accounts.csv
```

## Format of input
|heading|value|
|---|---|
|**type**|`withdraw`, `deposit`, `dispute`, `resolve`, or `chargeback`|
|**client**|a `u16` identifier|
|**tx**|a `u32` identifier|
|**amount**|a positive number containing up to 4 decimal places|

### Deposits
Increases the client's available funds by `amount`.

### Withdrawals
Decreases the client's available funds by `amount`.

### Dispute
Decreases the client's available funds by `amount` and increases held funds by `amount`. Total funds remain the same.

### Resolution
Increases the client's available funds by `amount` and decreases held funds by `amount`. Total funds remain the same.

### Chargeback
Decreases the client's held funds by `amount`. Total funds decrease.

## Output
|heading|value|
|---|---|
|client|a `u16` identifier|
|available|a real number containing up to 4 decimal places|
|held|a real number containing up to 4 decimal places|
|total|a real number containing up to 4 decimal places|
|locked|`true` or `false`|