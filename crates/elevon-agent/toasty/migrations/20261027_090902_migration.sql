ALTER TABLE "deployments" DROP COLUMN "web_domain";
-- #[toasty::breakpoint]
ALTER TABLE "deployments" DROP COLUMN "static_web_domain";
-- #[toasty::breakpoint]
ALTER TABLE "deployments" ADD COLUMN "domain" TEXT;
