ALTER TABLE "deployments" ADD COLUMN "static_web" BOOLEAN;
-- #[toasty::breakpoint]
ALTER TABLE "deployments" ADD COLUMN "static_web_index" TEXT;
-- #[toasty::breakpoint]
ALTER TABLE "deployments" ADD COLUMN "static_web_path" TEXT;
-- #[toasty::breakpoint]
ALTER TABLE "deployments" ADD COLUMN "static_web_domain" TEXT;
-- #[toasty::breakpoint]
ALTER TABLE "deployments" ADD COLUMN "static_web_directory" TEXT;
-- #[toasty::breakpoint]
ALTER TABLE "deployment_options" ADD COLUMN "healthcheck_start_period" INTEGER NOT NULL DEFAULT 5;
