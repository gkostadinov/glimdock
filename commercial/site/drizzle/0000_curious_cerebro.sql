CREATE TABLE `orders` (
	`session_id` text PRIMARY KEY NOT NULL,
	`reference` text NOT NULL,
	`payment_status` text NOT NULL,
	`quantity` integer NOT NULL,
	`country` text NOT NULL,
	`amount` integer NOT NULL,
	`created_at` integer NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `orders_reference_unique` ON `orders` (`reference`);--> statement-breakpoint
CREATE TABLE `withdrawals` (
	`id` text PRIMARY KEY NOT NULL,
	`reference` text NOT NULL,
	`email` text NOT NULL,
	`name` text NOT NULL,
	`submitted_at` integer NOT NULL,
	`acknowledged_at` integer
);
