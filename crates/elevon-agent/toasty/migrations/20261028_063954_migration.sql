PRAGMA foreign_keys = OFF;
-- #[toasty::breakpoint]
CREATE TABLE "_toasty_new_deployments" (
    "id" BLOB NOT NULL,
    "app_id" BLOB NOT NULL,
    "image_ref" TEXT,
    "container_id" TEXT,
    "domain" TEXT,
    "web" BOOLEAN,
    "web_port" INTEGER,
    "static_web" BOOLEAN,
    "static_web_directory" TEXT,
    "static_web_index" TEXT,
    "static_web_path" TEXT,
    "status" TEXT NOT NULL CHECK ("status" IN ('pending', 'active', 'drained', 'failed', 'pruned')),
    "created_at" TEXT NOT NULL,
    "updated_at" TEXT NOT NULL,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
INSERT INTO "_toasty_new_deployments" ("id", "app_id", "image_ref", "container_id", "domain", "web", "web_port", "static_web", "static_web_directory", "static_web_index", "static_web_path", "status", "created_at", "updated_at") SELECT "id", "app_id", "image_ref", "container_id", "domain", "web", "web_port", "static_web", "static_web_directory", "static_web_index", "static_web_path", "status", "created_at", "updated_at" FROM "deployments";
-- #[toasty::breakpoint]
DROP TABLE "deployments";
-- #[toasty::breakpoint]
ALTER TABLE "_toasty_new_deployments" RENAME TO "deployments";
-- #[toasty::breakpoint]
PRAGMA foreign_keys = ON;
