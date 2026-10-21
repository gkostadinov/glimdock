CREATE TABLE `interest_rate_limits` (
	`ip_hash` text PRIMARY KEY NOT NULL,
	`request_count` integer DEFAULT 0 NOT NULL,
	`expires_at` integer NOT NULL
);
--> statement-breakpoint
CREATE INDEX `interest_rate_expiry` ON `interest_rate_limits` (`expires_at`);--> statement-breakpoint
CREATE TABLE `interest_signups` (
	`id` text PRIMARY KEY NOT NULL,
	`email` text NOT NULL,
	`quantity` integer NOT NULL,
	`country` text NOT NULL,
	`body_color` text DEFAULT 'Purple' NOT NULL,
	`stand_color` text DEFAULT 'White' NOT NULL,
	`consent_version` text NOT NULL,
	`consented_at` integer NOT NULL,
	`created_at` integer NOT NULL,
	`confirmed_at` integer,
	`confirmation_sent_at` integer,
	`attempt_count` integer DEFAULT 0 NOT NULL,
	`next_attempt_at` integer DEFAULT 0 NOT NULL,
	`last_requested_at` integer NOT NULL,
	`expires_at` integer NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `interest_signups_email_unique` ON `interest_signups` (`email`);--> statement-breakpoint
CREATE INDEX `interest_delivery_due` ON `interest_signups` (`confirmation_sent_at`,`next_attempt_at`);--> statement-breakpoint
CREATE INDEX `interest_expiry` ON `interest_signups` (`expires_at`);