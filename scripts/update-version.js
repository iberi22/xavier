import fs from 'fs';
import path from 'path';

const ROOT_PACKAGE = './package.json';
const PANEL_UI_PACKAGE = './panel-ui/package.json';
const CARGO_TOML = './Cargo.toml';

// 1. Read root version
const rootPkg = JSON.parse(fs.readFileSync(ROOT_PACKAGE, 'utf-8'));
const currentVersion = rootPkg.version;
console.log(`Syncing version: ${currentVersion} across packages...`);

// 2. Update panel-ui/package.json
try {
  const panelPkg = JSON.parse(fs.readFileSync(PANEL_UI_PACKAGE, 'utf-8'));
  if (panelPkg.version !== currentVersion) {
    panelPkg.version = currentVersion;
    fs.writeFileSync(PANEL_UI_PACKAGE, JSON.stringify(panelPkg, null, 2) + '\n');
    console.log(`✅ Updated ${PANEL_UI_PACKAGE} to ${currentVersion}`);
  }
} catch (e) {
  console.error(`Failed to update ${PANEL_UI_PACKAGE}`, e);
}

// 3. Update Cargo.toml
try {
  let cargoContent = fs.readFileSync(CARGO_TOML, 'utf-8');
  // Reemplaza version = "x.x.x" en la sección [package]
  const updatedCargo = cargoContent.replace(/\[package\]\n([\s\S]*?)version = ".*?"/g, `[package]\n$1version = "${currentVersion}"`);
  if (cargoContent !== updatedCargo) {
    fs.writeFileSync(CARGO_TOML, updatedCargo);
    console.log(`✅ Updated ${CARGO_TOML} to ${currentVersion}`);
  }
} catch (e) {
  console.error(`Failed to update ${CARGO_TOML}`, e);
}

console.log('✅ Version sync complete.');
