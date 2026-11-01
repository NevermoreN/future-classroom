from pathlib import Path
R=Path('/tmp/fc-round68-production-https');W=R/'worktree';E=R/'evidence/b-source-before'
def edit(name,fn):
 p=W/name;q=E/name;q.parent.mkdir(parents=True,exist_ok=True)
 if not q.exists():q.write_bytes(p.read_bytes())
 p.write_text(fn(p.read_text()))
def storage(s):
 s=s.replace('fn configuration(value: &Option<HttpsConfiguration>) -> Value {','fn configuration(value: &Option<HttpsConfiguration>) -> Value {')
 s=s.replace('"certificate_store":"CURRENT_USER_MY","thumbprint_sha1":c.certificate.to_hex()})','"certificate_store":"CURRENT_USER_MY","thumbprint_sha1":c.certificate.to_hex(),\n        "source_networks":c.source_networks.as_ref().map(|p| p.networks().iter().map(|n| n.as_str()).collect::<Vec<_>>())})',1)
 s=s.replace('fn read_configuration(value: &Value) -> Result<Option<HttpsConfiguration>, StorageError> {','fn read_configuration(value: &Value, format: u64) -> Result<Option<HttpsConfiguration>, StorageError> {')
 s=s.replace('''    codec::object_fields(
        value,
        &["host", "port", "certificate_store", "thumbprint_sha1"],
    )?;''','''    if format == 1 {
        codec::object_fields(value, &["host", "port", "certificate_store", "thumbprint_sha1"])?;
    } else {
        codec::object_fields(value, &["host", "port", "certificate_store", "thumbprint_sha1", "source_networks"])?;
    }''',1)
 s=s.replace('''    HttpsConfiguration::new(
        host,''','''    let mut result = HttpsConfiguration::new(
        host,''',1)
 s=s.replace('''    .map(Some)
    .map_err(|_| StorageError::Database)
}''','''    .map_err(|_| StorageError::Database)?;
    if format == 2 && !value["source_networks"].is_null() {
        let rows = value["source_networks"].as_array().ok_or(StorageError::Database)?;
        if rows.is_empty() || rows.len() > 16 { return Err(StorageError::Database); }
        let mut networks = Vec::with_capacity(rows.len());
        for row in rows {
            let text = row.as_str().ok_or(StorageError::Database)?;
            let network = HttpsSourceNetwork::parse(text).map_err(|_| StorageError::Database)?;
            if network.as_str() != text { return Err(StorageError::Database); }
            networks.push(network);
        }
        result.source_networks = Some(HttpsSourceNetworks::new(networks).map_err(|_| StorageError::Database)?);
    }
    Ok(Some(result))
}''',1)
 s=s.replace('json!({"format":1,"revision":value.revision.get(),"configuration":configuration(&value.configuration)})','json!({"format":2,"revision":value.revision.get(),"configuration":configuration(&value.configuration)})',1)
 s=s.replace('''    if codec::number(value, "format")? != 1 {
        return Err(StorageError::Database);
    }
    let settings = HttpsSettings {''','''    let format = codec::number(value, "format")?;
    if !matches!(format, 1 | 2) { return Err(StorageError::Database); }
    let settings = HttpsSettings {''',1)
 s=s.replace('configuration: read_configuration(&value["configuration"])?','configuration: read_configuration(&value["configuration"], format)?',1)
 s=s.replace('''    hash.update(codec::bytes(&configuration(&request.configuration))?);''','''    let mut canonical = configuration(&request.configuration);
    // Preserve exact old command fingerprints when no policy was supplied.
    // A configured policy is new command payload and must match in full.
    if request.configuration.as_ref().is_some_and(|c| c.source_networks.is_none()) {
        canonical.as_object_mut().ok_or(StorageError::Database)?.remove("source_networks");
    }
    hash.update(codec::bytes(&canonical)?);''',1)
 return s
edit('crates/fc-storage/src/business/https_settings.rs',storage)
edit('crates/fc-storage/src/business/https_settings/tests.rs',lambda s:s.replace('old["format"] = json!(2);','old["format"] = json!(3);')+'''
#[test]
fn source_network_policy_roundtrips_current_installation_without_backup_export() {
    let mut f = Fixture::new();
    let base = f.read();
    let mut configured = config("board.school", 8443);
    configured.source_networks = Some(HttpsSourceNetworks::parse("192.168.10.0/24,2001:db8::/64").unwrap());
    let original = request(501, &base, Some(configured));
    let saved = f.save(original.clone()).unwrap();
    assert_eq!(f.save(original.clone()).unwrap(), saved);
    let mut changed = original;
    changed.configuration.as_mut().unwrap().source_networks = Some(HttpsSourceNetworks::parse("192.168.20.0/24").unwrap());
    assert_eq!(f.save(changed), Err(HttpsSettingsRepositoryError::IdempotencyConflict));
    f.reopen();
    assert_eq!(f.read(), saved);
    assert_eq!(settings(&saved.settings)["format"], json!(2));
}
#[test]
fn legacy_https_configuration_is_readable_but_has_no_invented_network_policy() {
    let legacy = json!({"format":1,"revision":1,"configuration":{
        "host":"board.school","port":443,"certificate_store":"CURRENT_USER_MY","thumbprint_sha1":"ab".repeat(20)}});
    let decoded = read_settings(&legacy).unwrap();
    assert!(decoded.configuration.as_ref().unwrap().source_networks.is_none());
    assert_eq!(read_settings(&settings(&decoded)).unwrap(), decoded);
    let mut bad = settings(&decoded);
    bad["configuration"]["source_networks"] = json!(["0.0.0.0/0"]);
    assert!(read_settings(&bad).is_err());
    bad["configuration"]["source_networks"] = json!([]);
    assert!(read_settings(&bad).is_err());
}
''')
