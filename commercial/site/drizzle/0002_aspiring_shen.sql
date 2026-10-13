ALTER TABLE `withdrawals` ADD `attempt_count` integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE `withdrawals` ADD `next_attempt_at` integer DEFAULT 0 NOT NULL;