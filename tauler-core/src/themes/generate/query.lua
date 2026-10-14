-- Answers a list of queries against one loaded colour scheme, as JSON `{ query = "#rrggbb" }`.
-- A query with no answer is left out.
--
-- Run by `cargo run -p tauler-core --example build_themes -- --queries`, which sets:
--   RTP      checkouts to add to the runtimepath, colon-separated
--   CS       the colour scheme to load
--   BGMODE   `dark` or `light`, a hint for schemes that branch on &background
--   SETUP    Lua to run before the scheme loads, or empty (a scheme that must be configured to
--            answer: a terminal-colour switch, an opaque background)
--   QUERIES  a JSON array of query strings (see queries.yaml for the grammar)
--
-- DECISION: this evaluates and nothing else. Which token a query belongs to, which mode's column
--   to read and what to do about a missing answer are the caller's, so the whole policy is the
--   table in queries.yaml and the few lines of Rust that read it.
for path in vim.env.RTP:gmatch("[^:]+") do
  vim.opt.rtp:append(path)
end
vim.o.termguicolors = true
vim.o.background = vim.env.BGMODE
if vim.env.SETUP and vim.env.SETUP ~= "" then
  local chunk, why = load(vim.env.SETUP)
  assert(chunk, "setup: " .. tostring(why))
  chunk()
end
local ok, err = pcall(vim.cmd.colorscheme, vim.env.CS)
if not ok then
  io.stderr:write("colorscheme " .. vim.env.CS .. ": " .. tostring(err) .. "\n")
  os.exit(1)
end

local function hex(n)
  return string.format("#%06x", n)
end

local function answer(query)
  local var = query:match("^g:(.+)$")
  if var then
    return vim.g[var]
  end
  -- `Group.attr`; a group name may itself contain dots (`@function.call`), so split at the last.
  local group, attr = query:match("^(.+)%.(%a+)$")
  if not group then
    error("not a query: " .. query)
  end
  local value = vim.api.nvim_get_hl(0, { name = group, link = false })[attr]
  return value and hex(value) or nil
end

local out = {}
for _, query in ipairs(vim.json.decode(vim.env.QUERIES)) do
  out[query] = answer(query)
end
io.stdout:write(vim.json.encode(out))
