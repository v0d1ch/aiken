# Changelog

## [next] - 2023-MM-DD

### Added

- **aiken**: new command `blueprint convert`

### Changed

- **aiken-project**: tests filtering with `-m` during check now happens in `Project::collect_tests`
- **aiken-project**: fixed generation of blueprints for recursive and mutually recursive data-types

- **aiken-lang**: block `Data` and `String` from unifying when casting
- **aiken-lang**: remove ability for a type with many variants with matching field labels and types to support field access
- **aiken-lang**: various uplc code gen fixes
- **aiken-lang**: update todo warning to include type
- **aiken-lang**: `|>` operator can now be formatted as a single (short) line or forced over multiline in a flexible manner
- **aiken-lang**: the compiler now provides better feedback for type holes (i.e. `_`) in type annotations
- **aiken-lang**: assignment and clause guard are now always formatted on a new line
- **aiken-lang**: unused let-bindings are now fully removed from generated code and discarded unused let-binding now raise a warning
- **aiken-lang**: support multi-clause patterns (only as a syntactic sugar)
- **aiken-lang**: fix lexer panic when parsing too large (> u32) tuple-indexes

## [v0.0.29] - 2023-MM-DD

### Added

- **aiken-project**: new dep rayon for parallel test execution
- **aiken**: new blueprint command
- **aiken-lang**: new syntax for defining validators
- **aiken**: new address command for deriving addresses out of `plutus.json`
- **aiken-lang**: Add missing Plutus builtins to Aiken's lang.
- **aiken**: fancy nix stuff
- **aiken-lsp**: go to definition
- **aiken-lsp**: docs on hover
- **aiken-lsp**: enable compiler a project

### Changed

- **aiken-lang**: `assert` renamed to `expect`
- **aiken-lang**: new syntax for strings and byte array literals
- **aiken-lang**: lots of code gen improvements
- **aiken-lang**: validator checks now happen during infer instead of in project
- **aiken-lang**: fixed unicode parsing
- **aiken-lang**: update default costs models
- **aiken-lang**: Use variable-length threshold for levenshtein distance
- **aiken-project**: Move module name validation outside of type-checking
- **aiken-project**: Add 'plutusVersion' to blueprints

### Removed

- **aiken-project**: remove assets folder in favor of `plutus.json`
- **aiken-lang**: removed some unused constant related data types

## [v0.0.28] - 2023-01-06

### Added

N/A

### Changed

- **uplc**: Reward accounts are now correctly turned into script credentials in ScriptContext.
- **all**: bump pallas version to `v0.16.0`

### Removed

N/A

## [v0.0.27] - 2022-MM-DD

### Added

- **aiken-lang**: integrated unit tests

  Aiken now supports writing unit tests directly in source files using the new
  `test` keyword. Tests are functions with no arguments that are implicitly typed
  to `bool`. For example:

  ```gleam
  test foo () {
    1 + 1 == 2
  }
  ```

- **aiken**: new `--skip-tests` flag for the `check` command

### Changed

- **aiken**: `check` now also runs and reports on any `test` found in the project
- **aiken**: fix Plutus V1 `to_plutus_data()` for post-alonzo txout with no datum hash

### Removed

N/A

## [v0.0.26] - 2022-11-23

### Added

- **aiken-lsp**: handle `DidSaveTextDocument` notification
- **aiken-lsp**: convert errors into `lsp_types::Diagnostic`
- **aiken-lang**: doc comment parsing
- **aiken-lang**: code generation for pattern matching expressions
- **aiken-lang**: extended script context
- **aiken-lang**: added Option to builtins
- **aiken-lang**: properly handle record parsing and sugar in patterns

## [v0.0.25] - 2022-11-14

### Added

- **aiken**: new `lsp` command
- **aiken**: new `fmt` command
- **aiken**: `build` command now works and outputs assets
- **aiken**: validate project name on `aiken new`
- **aiken-lang**: formatter for `UntypedExpr`
- **aiken-lang**: uplc code gen
- **aiken-lang**: add `Data` to prelude
- **aiken-lang**: allow `Data` to unify with anything that's not in the prelude
- **aiken-project**: validate if validator function return bool
- **aiken-project**: validate if validator function has minimum number of arguments
- **aiken-lsp**: new crate that contains the aiken language server

### Changed

- **uplc**: `Converter::get_index` now takes the full name to provide better error messages for `Error::FreeUnique`

## [v0.0.24] - 2022-11-04

### Changed

- **uplc**: Sorted remaining structured in the ScriptContext (Value, Wdrl, (Ref) Inputs, Mint, Required signers, Data, Redeemers)

## [v0.0.23] - 2022-11-03

### Changed

- **uplc**: sort inputs for script context fixes an issue in lucid https://github.com/spacebudz/lucid/issues/109

## [v0.0.22] - 2022-10-31

### Added

- **aiken**: Fancy errors using [miette](https://github.com/zkat/miette)
- **aiken**: Typechecking
- **aiken**: Inject `aiken/builtin` module with some functions from `DefaultFunction` in UPLC directly exposed
- **aiken-lang**: add `infer` method to `UntypedModule` which returns a `TypedModule`
- **uplc**: Expose various Pallas primitives from UPLC to make constructing
  UPLC types possible for consumers

### Changed

- **aiken**: Project structure is now a bit different. See [examples/sample](https://github.com/aiken-lang/aiken/tree/main/examples/sample) for more

## [v0.0.21] - 2022-10-23

### Added

- **flat-rs**: New errors for debugging flat decoding issues

### Changed

- **uplc**: Fixed overflow issue by changing `i64` to `i128` in `BigInt::Int` instances
- **uplc**: Added `apply_params_to_script` function (applies params to script and serializes the new script).

## [v0.0.20] - 2022-10-17

### Added

- **aiken**: `Project` module which is responsible loading modules and running the compilation steps
- **aiken**: `UplcCommand::Flat` flip the cbor_hex if condition so that the correct logic runs when using the flag
- **uplc**: use i128 for `Constant::Integer`
- **flat-rs**: add support for i128 encode and decode
- **flat-rs**: add i128 zigzag function
