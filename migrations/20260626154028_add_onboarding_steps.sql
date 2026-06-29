-- Add migration script here
ALTER TABLE workspaces
ADD COLUMN onboarding_steps JSONB NOT NULL DEFAULT '[]'::jsonb;