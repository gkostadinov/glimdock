CREATE TABLE `checkout_attempts` (
	`intent` text PRIMARY KEY NOT NULL,
	`quantity` integer NOT NULL,
	`country` text NOT NULL,
	`terms_version` text NOT NULL,
	`seller_postal_address` text NOT NULL,
	`created_at` integer NOT NULL,
	`checkout_origin` text NOT NULL,
	`session_id` text
);
--> statement-breakpoint
CREATE UNIQUE INDEX `checkout_attempts_session_id_unique` ON `checkout_attempts` (`session_id`);--> statement-breakpoint
ALTER TABLE `orders` ADD `terms_version` text;--> statement-breakpoint
ALTER TABLE `orders` ADD `seller_postal_address` text;--> statement-breakpoint
ALTER TABLE `orders` ADD `confirmation_attempt_count` integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE `orders` ADD `confirmation_next_attempt_at` integer DEFAULT 0 NOT NULL;