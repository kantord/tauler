# tauler-configgen

Reads a two-document YAML schema (a flat list of node declarations, then a `minijinja`
template) and generates the JSX components a Tauler layout uses to write another program's
config file, with validated props. The crate knows no config format: the format's syntax
lives only in the schema's `pattern` strings and template.

Tauler imports a `.schema.yaml` file from a layout directly, so no generate step is needed.

Documentation: <https://kantord.github.io/tauler/docs/develop/config-generation/>
