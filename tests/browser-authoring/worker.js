import initAuthoring, {create_package, edit_package, create_bundle, edit_bundle, verify_bundle, create_database, edit_database, verify_database} from './authoring/ms_package_authoring_worker_check.js';
import initArchive, {BytePackage, ByteBundle, ByteInstaller} from './pkg/archive_wasm.js';

function assert(value, message) { if (!value) throw new Error(message); }
async function parity(bytes, filename) {
  const native = new Uint8Array(await (await fetch(`./${filename}`)).arrayBuffer());
  assert(bytes.length === native.length && bytes.every((b, i) => b === native[i]), `${filename} native/Worker parity`);
}
function verifyReader(reader, expected) {
  try {
    reader.validate();
    const entry = JSON.parse(reader.entries_json()).find(e => e.name === 'payload.txt');
    const payload = reader.read_entry(entry.id, 1024n * 1024n);
    assert(payload.length === expected.length && payload.every((b, i) => b === expected[i]), 'decoded payload parity');
  } finally {reader.free();}
}
function verify(bytes, expected) { verifyReader(new BytePackage(bytes, 1024n * 1024n), expected); }
function verifyBundleArchive(bytes, expected) {
  const bundle = new ByteBundle(bytes, 1024n * 1024n);
  try {
    bundle.validate();
    assert(JSON.parse(bundle.packages_json())[0].file_name === 'nested.msix', 'bundle declaration');
    verifyReader(bundle.select('nested.msix', 1024n * 1024n), expected);
  } finally {bundle.free();}
}
function verifyDatabaseArchive(bytes) {
  const reader = new ByteInstaller(bytes, 1024n * 1024n, 100);
  try {assert(JSON.parse(reader.tables_json()).includes('Custom'), 'archive-rs MSI custom table');}
  finally {reader.free();}
}
self.onmessage = async () => {
  try {
    await initAuthoring(); await initArchive();
    const payload = new TextEncoder().encode('worker authoring payload');
    const replacement = new TextEncoder().encode('edited worker payload');
    const bytes = create_package(payload, 1024n * 1024n);
    await parity(bytes, 'native.msix');
    verify(bytes, payload);
    const edited = edit_package(bytes, replacement);
    await parity(edited, 'native-edited.msix');
    verify(edited, replacement);
    const bundle = create_bundle(payload, 1024n * 1024n);
    await parity(bundle, 'native.msixbundle');
    verify_bundle(bundle, payload);
    verifyBundleArchive(bundle, payload);
    const editedBundle = edit_bundle(bundle, replacement);
    await parity(editedBundle, 'native-edited.msixbundle');
    verify_bundle(editedBundle, replacement);
    verifyBundleArchive(editedBundle, replacement);
    const database = create_database(payload, 1024n * 1024n);
    await parity(database, 'native.msi');
    verify_database(database, payload);
    verifyDatabaseArchive(database);
    const editedDatabase = edit_database(database, replacement);
    await parity(editedDatabase, 'native-edited.msi');
    verify_database(editedDatabase, replacement);
    verifyDatabaseArchive(editedDatabase);
    for (const create of [create_package, create_bundle, create_database]) {
      let failed = false;
      try {create(payload, 1n);} catch (_) {failed = true;}
      assert(failed, 'actual payload limit enforcement');
    }
    self.postMessage({ok:true,checks:['APPX native create/edit parity','archive-rs reopen and payload bytes','bundle native create/edit parity','outer and nested integrity and payload bytes','MSI database native create/edit parity','custom MSI table and stream preservation','APPX/bundle/MSI limit failures'], cancellation:'synchronous calls; Worker termination is caller-owned'});
  } catch (error) {self.postMessage({ok:false,error:String(error)});}
};
