//! Closed readonly schema for this MSI family, not caller-supplied SQL.
pub(super) struct Table {
    pub(super) name: &'static str,
    pub(super) query: &'static str,
    pub(super) columns: &'static [(&'static str, &'static str)],
}
pub(super) const TABLES: &[Table] = &[
    Table {
        name: "ActionText",
        query: "SELECT * FROM `ActionText`",
        columns: &[
            ("Action", "s72"),
            ("Description", "L255"),
            ("Template", "L255"),
        ],
    },
    Table {
        name: "AdminExecuteSequence",
        query: "SELECT * FROM `AdminExecuteSequence`",
        columns: &[("Action", "s72"), ("Condition", "S255"), ("Sequence", "I2")],
    },
    Table {
        name: "AdminUISequence",
        query: "SELECT * FROM `AdminUISequence`",
        columns: &[("Action", "s72"), ("Condition", "S255"), ("Sequence", "I2")],
    },
    Table {
        name: "AdvtExecuteSequence",
        query: "SELECT * FROM `AdvtExecuteSequence`",
        columns: &[("Action", "s72"), ("Condition", "S255"), ("Sequence", "I2")],
    },
    Table {
        name: "AppSearch",
        query: "SELECT * FROM `AppSearch`",
        columns: &[("Property", "s72"), ("Signature_", "s72")],
    },
    Table {
        name: "Binary",
        query: "SELECT * FROM `Binary`",
        columns: &[("Name", "s72"), ("Data", "v0")],
    },
    Table {
        name: "Component",
        query: "SELECT * FROM `Component`",
        columns: &[
            ("Component", "s72"),
            ("ComponentId", "S38"),
            ("Directory_", "s72"),
            ("Attributes", "i2"),
            ("Condition", "S255"),
            ("KeyPath", "S72"),
        ],
    },
    Table {
        name: "CreateFolder",
        query: "SELECT * FROM `CreateFolder`",
        columns: &[("Directory_", "s72"), ("Component_", "s72")],
    },
    Table {
        name: "CustomAction",
        query: "SELECT * FROM `CustomAction`",
        columns: &[
            ("Action", "s72"),
            ("Type", "i2"),
            ("Source", "S72"),
            ("Target", "S255"),
        ],
    },
    Table {
        name: "Directory",
        query: "SELECT * FROM `Directory`",
        columns: &[
            ("Directory", "s72"),
            ("Directory_Parent", "S72"),
            ("DefaultDir", "l255"),
        ],
    },
    Table {
        name: "Error",
        query: "SELECT * FROM `Error`",
        columns: &[("Error", "i2"), ("Message", "L0")],
    },
    Table {
        name: "Feature",
        query: "SELECT * FROM `Feature`",
        columns: &[
            ("Feature", "s38"),
            ("Feature_Parent", "S38"),
            ("Title", "L64"),
            ("Description", "L255"),
            ("Display", "I2"),
            ("Level", "i2"),
            ("Directory_", "S72"),
            ("Attributes", "i2"),
        ],
    },
    Table {
        name: "FeatureComponents",
        query: "SELECT * FROM `FeatureComponents`",
        columns: &[("Feature_", "s38"), ("Component_", "s72")],
    },
    Table {
        name: "File",
        query: "SELECT * FROM `File`",
        columns: &[
            ("File", "s72"),
            ("Component_", "s72"),
            ("FileName", "l255"),
            ("FileSize", "i4"),
            ("Version", "S72"),
            ("Language", "S20"),
            ("Attributes", "I2"),
            ("Sequence", "i4"),
        ],
    },
    Table {
        name: "Icon",
        query: "SELECT * FROM `Icon`",
        columns: &[("Name", "s72"), ("Data", "v0")],
    },
    Table {
        name: "IniFile",
        query: "SELECT * FROM `IniFile`",
        columns: &[
            ("IniFile", "s72"),
            ("FileName", "l255"),
            ("DirProperty", "S72"),
            ("Section", "l255"),
            ("Key", "l255"),
            ("Value", "l255"),
            ("Action", "i2"),
            ("Component_", "s72"),
        ],
    },
    Table {
        name: "InstallExecuteSequence",
        query: "SELECT * FROM `InstallExecuteSequence`",
        columns: &[("Action", "s72"), ("Condition", "S255"), ("Sequence", "i2")],
    },
    Table {
        name: "InstallUISequence",
        query: "SELECT * FROM `InstallUISequence`",
        columns: &[("Action", "s72"), ("Condition", "S255"), ("Sequence", "I2")],
    },
    Table {
        name: "LaunchCondition",
        query: "SELECT * FROM `LaunchCondition`",
        columns: &[("Condition", "s255"), ("Description", "l255")],
    },
    Table {
        name: "Media",
        query: "SELECT * FROM `Media`",
        columns: &[
            ("DiskId", "i2"),
            ("LastSequence", "i4"),
            ("DiskPrompt", "L64"),
            ("Cabinet", "S255"),
            ("VolumeLabel", "S32"),
            ("Source", "S72"),
        ],
    },
    Table {
        name: "MsiFileHash",
        query: "SELECT * FROM `MsiFileHash`",
        columns: &[
            ("File_", "s72"),
            ("Options", "i2"),
            ("HashPart1", "i4"),
            ("HashPart2", "i4"),
            ("HashPart3", "i4"),
            ("HashPart4", "i4"),
        ],
    },
    Table {
        name: "Property",
        query: "SELECT * FROM `Property`",
        columns: &[("Property", "s72"), ("Value", "l0")],
    },
    Table {
        name: "RegLocator",
        query: "SELECT * FROM `RegLocator`",
        columns: &[
            ("Signature_", "s72"),
            ("Root", "i2"),
            ("Key", "s255"),
            ("Name", "S255"),
            ("Type", "I2"),
        ],
    },
    Table {
        name: "Registry",
        query: "SELECT * FROM `Registry`",
        columns: &[
            ("Registry", "s72"),
            ("Root", "i2"),
            ("Key", "l255"),
            ("Name", "L255"),
            ("Value", "L0"),
            ("Component_", "s72"),
        ],
    },
    Table {
        name: "RemoveFile",
        query: "SELECT * FROM `RemoveFile`",
        columns: &[
            ("FileKey", "s72"),
            ("Component_", "s72"),
            ("FileName", "L255"),
            ("DirProperty", "s72"),
            ("InstallMode", "i2"),
        ],
    },
    Table {
        name: "RemoveIniFile",
        query: "SELECT * FROM `RemoveIniFile`",
        columns: &[
            ("RemoveIniFile", "s72"),
            ("FileName", "l255"),
            ("DirProperty", "S72"),
            ("Section", "l255"),
            ("Key", "l255"),
            ("Value", "L255"),
            ("Action", "i2"),
            ("Component_", "s72"),
        ],
    },
    Table {
        name: "ServiceControl",
        query: "SELECT * FROM `ServiceControl`",
        columns: &[
            ("ServiceControl", "s72"),
            ("Name", "l255"),
            ("Event", "i2"),
            ("Arguments", "L255"),
            ("Wait", "I2"),
            ("Component_", "s72"),
        ],
    },
    Table {
        name: "ServiceInstall",
        query: "SELECT * FROM `ServiceInstall`",
        columns: &[
            ("ServiceInstall", "s72"),
            ("Name", "s255"),
            ("DisplayName", "L255"),
            ("ServiceType", "i4"),
            ("StartType", "i4"),
            ("ErrorControl", "i4"),
            ("LoadOrderGroup", "S255"),
            ("Dependencies", "S255"),
            ("StartName", "S255"),
            ("Password", "S255"),
            ("Arguments", "S255"),
            ("Component_", "s72"),
            ("Description", "L255"),
        ],
    },
    Table {
        name: "Shortcut",
        query: "SELECT * FROM `Shortcut`",
        columns: &[
            ("Shortcut", "s72"),
            ("Directory_", "s72"),
            ("Name", "l128"),
            ("Component_", "s72"),
            ("Target", "s72"),
            ("Arguments", "S255"),
            ("Description", "L255"),
            ("Hotkey", "I2"),
            ("Icon_", "S72"),
            ("IconIndex", "I2"),
            ("ShowCmd", "I2"),
            ("WkDir", "S72"),
            ("DisplayResourceDLL", "S255"),
            ("DisplayResourceId", "I2"),
            ("DescriptionResourceDLL", "S255"),
            ("DescriptionResourceId", "I2"),
        ],
    },
    Table {
        name: "Signature",
        query: "SELECT * FROM `Signature`",
        columns: &[
            ("Signature", "s72"),
            ("FileName", "s255"),
            ("MinVersion", "S20"),
            ("MaxVersion", "S20"),
            ("MinSize", "I4"),
            ("MaxSize", "I4"),
            ("MinDate", "I4"),
            ("MaxDate", "I4"),
            ("Languages", "S255"),
        ],
    },
    Table {
        name: "Upgrade",
        query: "SELECT * FROM `Upgrade`",
        columns: &[
            ("UpgradeCode", "s38"),
            ("VersionMin", "S20"),
            ("VersionMax", "S20"),
            ("Language", "S255"),
            ("Attributes", "i4"),
            ("Remove", "S255"),
            ("ActionProperty", "s72"),
        ],
    },
];
pub(super) fn table(name: &str) -> Option<&'static Table> {
    TABLES.iter().find(|t| t.name == name)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_schema_rejects_unknown_identifiers_and_closes_complete_column_types() {
        assert_eq!(TABLES.len(), 31);
        for pair in TABLES.windows(2) {
            assert!(pair[0].name < pair[1].name);
        }
        for t in TABLES {
            assert!(!t.columns.is_empty() && t.columns.len() <= 32);
            assert_eq!(t.query, format!("SELECT * FROM `{}`", t.name));
        }
        for bad in [
            "Other",
            "Property`",
            "MsiEmbeddedChainer",
            "_Streams",
            "ServiceInstall;DROP",
        ] {
            assert!(table(bad).is_none());
        }
    }
}
