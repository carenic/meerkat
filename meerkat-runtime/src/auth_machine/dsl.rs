// AuthMachine production body is catalog-owned. Keep bridge/runtime mechanics
// outside this file; canonical lifecycle semantics live in the catalog DSL.
meerkat_machine_schema::auth_catalog_machine_dsl!("meerkat-runtime", "auth_machine::dsl");
