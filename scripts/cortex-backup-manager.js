#!/usr/bin/env node
/**
 * xavier-backup-manager.js
 * Gestor de backups do volume Xavier com timestamps
 * Uso: node xavier-backup-manager.js [backup|restore|list] [backend-name]
 */

const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const VOLUME = "xavier_data";
const BACKUP_DIR = "E:\\scripts-python\\xavier\\backup";
const XAVIER_IMAGE = "xavier:0.4.4";

function timestamp() {
  const d = new Date();
  return d.toISOString().replace(/[:.]/g, "-").slice(0, 19);
}

function getContainerMount() {
  try {
    const info = execSync(
      `docker inspect xavier-test --format "{{.Mounts}}" 2>&1`,
      { encoding: "utf-8" },
    );
    const match = info.match(/xavier_data.*?(\/[^,\s]+)/);
    return match ? match[1] : "/data";
  } catch {
    return "/data";
  }
}

function listBackups() {
  const files = fs
    .readdirSync(BACKUP_DIR)
    .filter((f) => f.startsWith("xavier_") && f.endsWith(".db"));
  console.log("\n=== Backups disponibles ===");
  files.sort().forEach((f) => {
    const parts = f.replace(".db", "").split("_");
    if (parts.length >= 5) {
      const date = parts.slice(2, 4).join("-");
      const time = parts.slice(4, 6).join(":");
      const backend = parts.slice(6).join("_").replace(/_/g, " ");
      console.log(`  ${f.replace(".db", "")}`);
      console.log(`    Fecha: ${date} ${time} | Backend: ${backend}`);
    }
  });
  console.log(`\nTotal: ${files.length} archivos\n`);
}

function createBackup(name) {
  const ts = timestamp();
  const mount = getContainerMount();
  const prefix = `xavier_memory_vec_${ts}_${name.replace(/\s+/g, "_")}`;
  const graphPrefix = `xavier_code_graph_${ts}_${name.replace(/\s+/g, "_")}`;

  console.log(`Creando backup: ${name} (${ts})`);

  const files = [
    {
      src: `${mount}/xavier_memory_vec.db`,
      dest: `${BACKUP_DIR}/${prefix}.db`,
    },
    { src: `${mount}/code_graph.db`, dest: `${BACKUP_DIR}/${graphPrefix}.db` },
  ];

  for (const f of files) {
    try {
      execSync(
        `docker run --rm -v ${VOLUME}:${mount} -v "${BACKUP_DIR}:/backup" ${XAVIER_IMAGE} cp ${f.src} /backup/${path.basename(f.dest)}`,
        { stdio: "inherit" },
      );
      const size = fs.statSync(f.dest).size;
      console.log(
        `  [OK] ${path.basename(f.dest)} (${(size / 1024).toFixed(1)}KB)`,
      );
    } catch (e) {
      console.log(`  [FAIL] ${f.src}: ${e.message}`);
    }
  }

  // Also save a JSON manifest
  const manifest = {
    timestamp: ts,
    backend: name,
    files: files.map((f) => ({
      name: path.basename(f.dest),
      size: fs.existsSync(f.dest) ? fs.statSync(f.dest).size : 0,
    })),
  };
  fs.writeFileSync(
    `${BACKUP_DIR}/manifest_${ts}_${name.replace(/\s+/g, "_")}.json`,
    JSON.stringify(manifest, null, 2),
  );
  console.log(
    `Manifiesto guardado: manifest_${ts}_${name.replace(/\s+/g, "_")}.json`,
  );
}

function restoreBackup(backupName) {
  console.log(`Restore de: ${backupName}`);

  let tsAndName = backupName;
  if (backupName.startsWith("xavier_memory_vec_")) {
    tsAndName = backupName.replace("xavier_memory_vec_", "");
  } else if (backupName.startsWith("xavier_code_graph_")) {
    tsAndName = backupName.replace("xavier_code_graph_", "");
  } else if (backupName.startsWith("manifest_")) {
    tsAndName = backupName.replace("manifest_", "").replace(".json", "");
  }
  if (tsAndName.endsWith(".db")) {
    tsAndName = tsAndName.slice(0, -3);
  }

  const mount = getContainerMount();
  let manifestPath = `${BACKUP_DIR}/manifest_${tsAndName}.json`;

  if (fs.existsSync(manifestPath)) {
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));

    const files = manifest.files.map((f) => {
      let target = `${mount}/xavier_memory_vec.db`;
      if (f.name.includes("code_graph")) {
        target = `${mount}/code_graph.db`;
      } else if (f.name.includes("memory_vec")) {
        target = `${mount}/xavier_memory_vec.db`;
      }
      return {
        src: `${BACKUP_DIR}/${f.name}`,
        dest: target,
        name: f.name,
      };
    });

    for (const f of files) {
      try {
        if (fs.existsSync(f.src)) {
          execSync(
            `docker run --rm -v ${VOLUME}:${mount} -v "${BACKUP_DIR}:/backup" ${XAVIER_IMAGE} cp /backup/${f.name} ${f.dest}`,
            { stdio: "inherit" },
          );
          console.log(`  [OK] Restored ${f.name} to ${f.dest}`);
        } else {
          console.log(`  [FAIL] Archivo no encontrado: ${f.src}`);
        }
      } catch (e) {
        console.log(`  [FAIL] ${f.name}: ${e.message}`);
      }
    }
  } else {
    console.log(
      `No se encontró el manifiesto para: ${tsAndName}. Intentando restaurar usando el nombre como prefijo...`,
    );
    const prefix = `xavier_memory_vec_${tsAndName}`;
    const graphPrefix = `xavier_code_graph_${tsAndName}`;

    const files = [
      { name: `${prefix}.db`, dest: `${mount}/xavier_memory_vec.db` },
      { name: `${graphPrefix}.db`, dest: `${mount}/code_graph.db` },
    ];

    for (const f of files) {
      const src = `${BACKUP_DIR}/${f.name}`;
      try {
        if (fs.existsSync(src)) {
          execSync(
            `docker run --rm -v ${VOLUME}:${mount} -v "${BACKUP_DIR}:/backup" ${XAVIER_IMAGE} cp /backup/${f.name} ${f.dest}`,
            { stdio: "inherit" },
          );
          console.log(`  [OK] Restored ${f.name} to ${f.dest}`);
        } else {
          console.log(`  [FAIL] Archivo no encontrado: ${src}`);
        }
      } catch (e) {
        console.log(`  [FAIL] ${f.name}: ${e.message}`);
      }
    }
  }
}

const cmd = process.argv[2] || "list";
const name = process.argv[3] || "unnamed";

if (cmd === "list") listBackups();
else if (cmd === "backup") createBackup(name);
else if (cmd === "restore") restoreBackup(name);
else
  console.log(
    "Uso: node xavier-backup-manager.js [backup|restore|list] [nombre-backend]",
  );
