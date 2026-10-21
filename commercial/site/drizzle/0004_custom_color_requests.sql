ALTER TABLE `checkout_attempts` ADD `body_color` text DEFAULT 'Purple' NOT NULL;--> statement-breakpoint
ALTER TABLE `checkout_attempts` ADD `stand_color` text DEFAULT 'White' NOT NULL;--> statement-breakpoint
ALTER TABLE `orders` ADD `body_color` text DEFAULT 'Purple' NOT NULL;--> statement-breakpoint
ALTER TABLE `orders` ADD `stand_color` text DEFAULT 'White' NOT NULL;