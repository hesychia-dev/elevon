PRAGMA foreign_keys = OFF;
-- #[toasty::breakpoint]
CREATE TABLE "_toasty_new_deployments" (
    "id" BLOB NOT NULL,
    "app_id" BLOB NOT NULL,
    "image_ref" TEXT NOT NULL,
    "container_id" TEXT,
    "routing" BOOLEAN,
    "routing_domain" TEXT,
    "routing_path_prefix" TEXT,
    "routing_port" INTEGER,
    "routing_container_port" INTEGER,
    "routing_static_files" BOOLEAN,
    "routing_static_files_directory" TEXT,
    "routing_static_files_index" TEXT,
    "status" TEXT NOT NULL CHECK ("status" IN ('pending', 'active', 'drained', 'failed', 'pruned')),
    "created_at" TEXT NOT NULL,
    "updated_at" TEXT NOT NULL,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
INSERT INTO "_toasty_new_deployments" ("id", "app_id", "image_ref", "container_id", "routing_domain", "routing_path_prefix", "routing_port", "routing_static_files_directory", "routing_static_files_index", "status", "created_at", "updated_at") SELECT "id", "app_id", "image_ref", "container_id", "domain", "static_web_path", "web_port", "static_web_directory", "static_web_index", "status", "created_at", "updated_at" FROM "deployments";
-- #[toasty::breakpoint]
DROP TABLE "deployments";
-- #[toasty::breakpoint]
ALTER TABLE "_toasty_new_deployments" RENAME TO "deployments";
-- #[toasty::breakpoint]
PRAGMA foreign_keys = ON;
