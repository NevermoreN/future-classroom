# Second independent chainer fixture: exact HKCU32 marker

This new directory preserves the first preparation and actual first failure.
First MSI had an empty Feature, so Windows Installer selected REMOVE=ALL and
skipped the chainer. First /i exit0 was not a Join/End success. Root separately
proved first Product/related/cache/client absent and removed its owned root;
the shared SCM service was retained.

C# template is byte-identical to first (c5cdfbce...). With the same root-chosen
nonce/report-root/owner constants, generated source remains cb23dc24... and the
already compiled GUI x86 EXE can be reused only with its exact actual receipt
and SHA39af6ce7.... No source synchronization barrier or normal product code
change is added.

Use `chainer-probe.py prepare --nonce --windows-report-root --owner-sid --output-dir`
with a fresh LOCAL output directory. Do not overwrite first spec/WXS/MSI.
The previous spec interface gains exactly componentCode, registryKey,
registryValue=OwnedMarker, registryData=1, registryView=Registry32. ProductCode
and UpgradeCode retain the same nonce mapping (root must prove them absent
again); PackageCode uses a new second-registry role, so changed MSI media does
not reuse the first PackageCode. No operational prepare/build/Windows has run
from this directory.

Authoring adds one Component ChainerRegistryProbe, Win64=no, Attributes4 registry
keypath, Directory LocalAppDataFolder; one HKCU RegistryValue at exactly
Software\FutureClassroom-Test-ChainerProbe-<nonce>, Name OwnedMarker, DWORD1,
KeyPath=yes; and exactly one FeatureComponents reference. This is the same
registry-keypath convention already independently tested, using an unrelated
nonce namespace. No file, Run, Upgrade, service, shortcut, external CA, normal
product gate/Pending or school data is admitted. Registry mutation is a real
fixture effect; its exact whole nonce key must be absent before and after.

The checker still requires all original final table exports, exact neutral
virtual codepage, x86 MSI500 summary, exact Binary bytes, csc source-before/after
and actual compiler receipt, GUI2/ILONLY/32BITREQUIRED PE, closed literal chainer
argv and all standard-action/auxiliary rules. It now requires exactly the three
new registry/component/FeatureComponents rows and exact Directory/Feature rows,
including RemoveRegistryValues and WriteRegistryValues normal actions. Root2,
64-bit attribute260, other key/value/data/component or detached Feature rejects.
All other first exclusions stay strict. `check-exports` CLI is unchanged.

C# pre-join/result protocols and its original compiler flags/receipt are unchanged.
Root alone builds and executes. Follow the new chainer-runtime-preparation-second
controller's 120-second total/20-second cleanup reserve, quiet /i, exact HKCU32
marker checks and try/finally formal /x if any registration or marker remains.
No hand-deletion of registration, marker or MSI cache is authorized. The fixture
excludes chainer execution during REMOVE=ALL cleanup. Failure/unknown API or
process exit stays failed/unknown even if cleanup passes.

This remains independent .NET/MSI API evidence, not Rust guardian acceptance,
Begin-to-transaction proof, arbitrary external MSI/transform resistance or any
Windows7/10 runtime claim. Root's shared msiserver service must never be waited
or killed; only exact test client/helper return is required. Root owns all actual
Windows operations and separate final cache inventory.
