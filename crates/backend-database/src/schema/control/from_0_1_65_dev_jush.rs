//! From the control schema of before the product took the name Jush: what it
//! stores under the earlier names takes the new ones.
//!
//! - A Cloud's home moves from `/home/demi` to `/home/jush`, which its base
//!   now names, and so do the paths under it that the Cloud's records,
//!   projects, worktrees, workflows, conversations and pins keep.
//! - A path in a runner's installation, `~/.demi/runner/…` or
//!   `~/.demi/instances/…`, as a conversation's scratch is, or in a Cloud's
//!   `~/.demi`, moves to `~/.jush`, where the runner moves the directory, in
//!   the conversations' pins, directories and work documents.
//! - The command packages a device reported, such as `demi.browser`, are
//!   named `jush.browser`.
//! - A schedule's command, which runs unattended, names the runner's
//!   directories under `~/.jush` and the variables a job gets as `JUSH_*`;
//!   a variable of the user's own keeps its name; a Cloud conversation's
//!   names its home `/home/jush`.
//!
//! A row that names its device moves a path under `/home/demi` only when the
//! device is a Cloud, since a paired device's user may be named so; a
//! conversation's last switch and a plugin's document name none, and take
//! such a path to be a Cloud's. Everything else that holds the earlier
//! name, such as a title or a message, is history and stays as written.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONTROL_FROM_0_1_65_DEV_JUSH);

const CONTROL_FROM_0_1_65_DEV_JUSH: &str = "
UPDATE devices SET home = '/home/jush' || substr(home, 11)
WHERE (home = '/home/demi' OR home LIKE '/home/demi/%') AND kind = 'managed';
UPDATE projects SET path = '/home/jush' || substr(path, 11)
WHERE (path = '/home/demi' OR path LIKE '/home/demi/%') AND device_id IN (SELECT id FROM devices WHERE kind = 'managed');
UPDATE directory_worktrees SET path = '/home/jush' || substr(path, 11)
WHERE (path = '/home/demi' OR path LIKE '/home/demi/%') AND device_id IN (SELECT id FROM devices WHERE kind = 'managed');
UPDATE directory_worktrees SET worktree_path = '/home/jush' || substr(worktree_path, 11)
WHERE (worktree_path = '/home/demi' OR worktree_path LIKE '/home/demi/%') AND device_id IN (SELECT id FROM devices WHERE kind = 'managed');
UPDATE device_workflows SET directory = '/home/jush' || substr(directory, 11)
WHERE (directory = '/home/demi' OR directory LIKE '/home/demi/%') AND device_id IN (SELECT id FROM devices WHERE kind = 'managed');
UPDATE conversations SET target_path = '/home/jush' || substr(target_path, 11)
WHERE (target_path = '/home/demi' OR target_path LIKE '/home/demi/%') AND (target_kind = 'cloud' OR target_device_id IN (SELECT id FROM devices WHERE kind = 'managed'));
UPDATE conversations SET target_worktree_path = '/home/jush' || substr(target_worktree_path, 11)
WHERE (target_worktree_path = '/home/demi' OR target_worktree_path LIKE '/home/demi/%') AND (target_kind = 'cloud' OR target_device_id IN (SELECT id FROM devices WHERE kind = 'managed'));
UPDATE conversation_devices SET cwd = '/home/jush' || substr(cwd, 11)
WHERE (cwd = '/home/demi' OR cwd LIKE '/home/demi/%') AND device_id IN (SELECT id FROM devices WHERE kind = 'managed');
UPDATE conversation_pins SET path = '/home/jush' || substr(path, 11)
WHERE (path = '/home/demi' OR path LIKE '/home/demi/%') AND device_id IN (SELECT id FROM devices WHERE kind = 'managed');
UPDATE handoff_codes SET to_path = '/home/jush' || substr(to_path, 11)
WHERE to_path = '/home/demi' OR to_path LIKE '/home/demi/%';
UPDATE conversations SET last_switch =
  replace(replace(last_switch, '\"/home/demi\"', '\"/home/jush\"'), '\"/home/demi/', '\"/home/jush/')
WHERE last_switch LIKE '%\"/home/demi%';
UPDATE conversation_work SET document =
  replace(replace(replace(document, '\"/home/demi\"', '\"/home/jush\"'), '\"/home/demi/', '\"/home/jush/'),
    'file:///home/demi/', 'file:///home/jush/')
WHERE document LIKE '%/home/demi%';
UPDATE conversation_pins SET path =
  replace(replace(replace(replace(replace(path,
    '/.demi/runner/', '/.jush/runner/'), '/.demi/instances/', '/.jush/instances/'),
    '\\.demi\\runner\\', '\\.jush\\runner\\'), '\\.demi\\instances\\', '\\.jush\\instances\\'),
    '/home/jush/.demi/', '/home/jush/.jush/')
WHERE path LIKE '%.demi%';
UPDATE conversation_devices SET cwd =
  replace(replace(replace(replace(replace(cwd,
    '/.demi/runner/', '/.jush/runner/'), '/.demi/instances/', '/.jush/instances/'),
    '\\.demi\\runner\\', '\\.jush\\runner\\'), '\\.demi\\instances\\', '\\.jush\\instances\\'),
    '/home/jush/.demi/', '/home/jush/.jush/')
WHERE cwd LIKE '%.demi%';
UPDATE conversation_work SET document =
  replace(replace(replace(replace(replace(document,
    '/.demi/runner/', '/.jush/runner/'), '/.demi/instances/', '/.jush/instances/'),
    '\\\\.demi\\\\runner\\\\', '\\\\.jush\\\\runner\\\\'), '\\\\.demi\\\\instances\\\\', '\\\\.jush\\\\instances\\\\'),
    '/home/jush/.demi/', '/home/jush/.jush/')
WHERE document LIKE '%.demi%';
UPDATE devices SET installed = replace(installed, '\"package\":\"demi.', '\"package\":\"jush.')
WHERE installed LIKE '%\"package\":\"demi.%';
UPDATE schedules SET trigger =
  replace(replace(replace(replace(replace(replace(trigger,
    '~/.demi/', '~/.jush/'), '/.demi/runner/', '/.jush/runner/'), '/.demi/instances/', '/.jush/instances/'),
    'DEMI_SCRATCH', 'JUSH_SCRATCH'), 'DEMI_JOB_ID', 'JUSH_JOB_ID'), 'DEMI_HOME', 'JUSH_HOME')
WHERE trigger LIKE '%.demi/%' OR trigger LIKE '%DEMI\\_%' ESCAPE '\\';
UPDATE schedules SET trigger = replace(replace(trigger, '/home/demi/', '/home/jush/'), '/home/demi\"', '/home/jush\"')
WHERE trigger LIKE '%/home/demi%' AND conversation_id IN (
  SELECT id FROM conversations
  WHERE target_kind = 'cloud' OR target_device_id IN (SELECT id FROM devices WHERE kind = 'managed')
);
";
