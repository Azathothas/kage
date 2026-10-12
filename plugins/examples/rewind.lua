-- rewind.lua - file checkpoints around the built-in /undo and /rewind.
--
-- kage's built-in /undo and /rewind (or esc esc) move the conversation
-- back, but they leave your working tree as it is. This plugin adds
-- the file half: it snapshots tracked changes with git at every turn
-- end, and its commands restore them.
--
-- Grant it in config.toml:
--
--   [plugins.capabilities]
--   rewind = ["session_write", "exec"]
--
-- session_write unlocks kage.session.entries (ordering checkpoints
-- against the conversation); exec unlocks kage.exec so file state can
-- be snapshotted and restored. Either capability may be withheld:
-- without session_write the plugin disables itself; without exec it
-- tracks nothing.
--
-- Commands:
--   /checkpoint-restore [at]  restore tracked file changes to the
--                             checkpoint of an earlier turn (entry-id
--                             prefix, or a picker when omitted). The
--                             state you leave behind goes to redo.
--   /redo                     re-apply the tracked changes the last
--                             /checkpoint-restore moved away from.
--                             Redo entries survive later turns.
--   /checkpoint-status        how many checkpoints / redo entries are
--                             held
--
-- Flow: run the built-in /undo or /rewind first, then
-- /checkpoint-restore to bring the files along.
--
-- Scope/limits: file snapshots use `git stash create`, so they cover
-- tracked changes only (not untracked files) and require a git work
-- tree. Redo entries reference unreferenced git stash commits, so
-- very old ones may be pruned by git's garbage collection.

local caps = kage.request_capabilities({ 'session_write', 'exec' })

if not caps.session_write then
    kage.notify('rewind: disabled (grant session_write in [plugins.capabilities])')
    return
end

local files = caps.exec

-- Per-turn checkpoints in chronological order: each is
-- { id = <entry id at turn end>, sha = <git stash sha or false> }.
-- `redo` holds stash shas of states a /checkpoint-restore moved away
-- from; it survives later turns so a prompt between restore and redo
-- does not wipe it.
local checkpoints = {}
local redo = {}

local warned_no_git = false
local function git(args)
    local ok, r = pcall(kage.exec, { cmd = 'git', args = args })
    if not ok then
        if not warned_no_git then
            warned_no_git = true
            kage.notify('rewind: git unavailable; file checkpoints disabled')
        end
        return nil, nil
    end
    if r.code ~= 0 then
        return nil, (r.stderr ~= '' and r.stderr or r.stdout)
    end
    return r.stdout, nil
end

local function in_git_repo()
    local out = git({ 'rev-parse', '--is-inside-work-tree' })
    return out ~= nil and out:find('true') ~= nil
end

local function snapshot()
    local out = git({ 'stash', 'create', 'kage-rewind checkpoint' })
    if out == nil then return false end
    local sha = out:gsub('%s+$', '')
    return sha ~= '' and sha or false
end

local function restore(sha)
    if not sha then return false end
    local _, err = git({ 'checkout', sha, '--', '.' })
    if err then
        kage.notify('rewind: file restore failed: ' .. err)
        return false
    end
    return true
end

local function last_entry_id()
    local entries = kage.session.entries()
    local last = entries[#entries]
    return last and last.id or nil
end

-- The newest checkpoint whose turn ended at or before `at`. Entry ids
-- are ULIDs, so a plain string compare orders them chronologically.
local function checkpoint_for(at)
    local best
    for _, c in ipairs(checkpoints) do
        if c.id <= at then best = c else break end
    end
    return best
end

kage.on('turn_end', function()
    if not files or not in_git_repo() then return end
    local id = last_entry_id()
    if not id then return end
    checkpoints[#checkpoints + 1] = { id = id, sha = snapshot() }
end)

kage.register_command({
    name = 'checkpoint-restore',
    description = 'Restore tracked files to an earlier turn checkpoint (pairs with /undo and /rewind)',
    args = {
        { name = 'at', kind = 'text', optional = true, hint = 'entry-id prefix of the turn end' },
    },
    handler = function(_, _, parsed)
        if not files then return 'restore unavailable (exec not granted)' end
        if #checkpoints == 0 then
            kage.notify('checkpoint-restore: no checkpoints yet')
            return 'nothing to restore'
        end

        local at = parsed.at
        if at == nil or at == '' then
            local entries = kage.session.entries()
            local ts_of = {}
            for _, e in ipairs(entries) do ts_of[e.id] = e.ts end
            local items = {}
            for _, c in ipairs(checkpoints) do
                local ts = ts_of[c.id]
                items[#items + 1] = {
                    label = string.format('turn end %s  %s',
                        ts and ts:sub(12, 19) or '????????', c.id:sub(1, 8)),
                    value = c.id,
                }
            end
            at = kage.ui.select('Restore files to', items)
            if at == nil then return 'cancelled' end
        end

        local cp = checkpoint_for(at)
        if not cp then
            kage.notify('checkpoint-restore: no checkpoint at or before ' .. at:sub(1, 8))
            return 'no such checkpoint'
        end
        if not cp.sha then
            kage.notify('checkpoint-restore: that turn recorded no tracked changes')
            return 'nothing tracked at that turn'
        end

        local pre = snapshot()
        if pre then redo[#redo + 1] = pre end
        if restore(cp.sha) then
            kage.notify('checkpoint-restore: restored tracked modifications')
            return 'restored to ' .. cp.id:sub(1, 8)
        end
        return 'restore failed'
    end,
})

kage.register_command({
    name = 'redo',
    description = 'Re-apply the tracked changes the last /checkpoint-restore moved away from (survives later prompts)',
    handler = function()
        if not files then return 'redo unavailable (exec not granted)' end
        local sha = table.remove(redo)
        if not sha then
            kage.notify('redo: nothing to redo')
            return 'nothing to redo'
        end
        if restore(sha) then
            kage.notify('redo: re-applied tracked changes')
            return 're-applied'
        end
        return 'redo failed'
    end,
})

kage.register_command({
    name = 'checkpoint-status',
    description = 'Show how many rewind checkpoints and redo entries are held',
    handler = function()
        return string.format('rewind: %d checkpoint(s), %d redo, files=%s',
            #checkpoints, #redo, tostring(files and true or false))
    end,
})
