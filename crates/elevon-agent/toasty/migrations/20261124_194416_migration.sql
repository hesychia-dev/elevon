ALTER TABLE "deployments" RENAME COLUMN "routing_port" TO "routing_config_inner_exposed_port";
-- #[toasty::breakpoint]
ALTER TABLE "deployments" RENAME COLUMN "routing_container_port" TO "routing_config_inner_internal_port";
-- #[toasty::breakpoint]
ALTER TABLE "deployments" DROP COLUMN "routing_static_files";
-- #[toasty::breakpoint]
ALTER TABLE "deployments" RENAME COLUMN "routing_static_files_directory" TO "routing_config_inner_directory";
-- #[toasty::breakpoint]
ALTER TABLE "deployments" RENAME COLUMN "routing_static_files_index" TO "routing_config_inner_index";
-- #[toasty::breakpoint]
ALTER TABLE "deployments" ADD COLUMN "routing_config" TEXT CHECK ("routing_config" IN ('server_routing', 'static_routing'));
