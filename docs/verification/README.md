# Verification evidence

Add dated reports tied to an exact commit or source-manifest hash. Record commands,
environment, success/failure, excluded scope, and artifact hashes where applicable.
Keep private diagnostics and recordings outside Git and reference them cautiously.
Generated logs live in the ignored `.redunar-build/reports/` directory. A committed
summary should describe the evidence and limitations without dumping private logs.
Historical results in TESTING remain available; new long-form reports belong here.
