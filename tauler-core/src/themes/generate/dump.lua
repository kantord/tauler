-- Prints what every colour scheme has to provide, as JSON: the background and foreground of
-- `Normal`, and the 16 terminal colours. Everything else a theme needs is derived from these
-- in Rust (`examples/build_themes/derive.rs`), the same way for every scheme.
--
-- Run by `cargo run -p tauler-core --example build_themes`, which sets:
--   RTP    a checkout of the scheme's repository, added to the runtimepath
--   CS     the colour scheme to load (`:colorscheme CS`)
--   BGMODE `dark` or `light` — a hint for &background, which schemes that ship both styles branch
--          on. A scheme that sets its own (tokyonight-day does) overrides it; the value printed is
--          the one in effect after the scheme loaded.
--
-- TERM: Seed — the (background, foreground, 16 ANSI colours) a theme is derived from. It is
--   what any terminal theme or Neovim scheme provides, which is why it is the input and the
--   surface ladder (`card`, `muted`, `border`...) is not: a scheme that ships a ladder
--   (base16 does) is the exception, and deriving one is what lets every scheme be handled alike.
--   Avoid: palette (a scheme's own word for its named colours, which the seed is only part of).
--
-- DECISION: read `Normal` and `terminal_color_0..15`, not the scheme's own palette module. They
--   exist in every scheme; a palette module (tokyonight's `colors.lua`) is per-scheme code, and
--   reading each would be the per-scheme mapping this replaced.
--
-- DECISION: a scheme that sets no `terminal_color_N` (catppuccin, by default) or only some of them
--   gets the rest from its highlight groups — red from `DiagnosticError`, green from
--   `DiagnosticOk`, and so on — and bright = normal; `fallback` is printed so a maintainer can see
--   it happened. Rejected: failing the workload; a quarter of the schemes tried needed this.
-- One or more checkouts, colon-separated: a scheme may need a library repository beside it.
for path in vim.env.RTP:gmatch("[^:]+") do
  vim.opt.rtp:append(path)
end
vim.o.termguicolors = true
vim.o.background = vim.env.BGMODE
local ok, err = pcall(vim.cmd.colorscheme, vim.env.CS)
if not ok then
  io.stderr:write("colorscheme " .. vim.env.CS .. ": " .. tostring(err) .. "\n")
  os.exit(1)
end

local function hex(n) return n and string.format("#%06x", n) or nil end
local function fg_of(group)
  return hex(vim.api.nvim_get_hl(0, { name = group, link = false }).fg)
end

local normal = vim.api.nvim_get_hl(0, { name = "Normal", link = false })
local bg, fg = hex(normal.bg), hex(normal.fg)
assert(bg and fg, vim.env.CS .. " does not set Normal's background and foreground")

local ansi = {}
for i = 0, 15 do ansi[i + 1] = vim.g["terminal_color_" .. i] end

-- Whatever the scheme left unset is filled in, and `fallback` says so: a missing hue comes from
-- the highlight group every scheme styles for it, black and white from the background and
-- foreground, and a missing bright variant is the normal one.
local fallback = false
local hues = {
  [2] = function() return fg_of("DiagnosticError") end,
  [3] = function() return fg_of("DiagnosticOk") or fg_of("String") end,
  [4] = function() return fg_of("DiagnosticWarn") end,
  [5] = function() return fg_of("Directory") or fg_of("Function") end,
  [6] = function() return fg_of("Keyword") or fg_of("Statement") end,
  [7] = function() return fg_of("Special") or fg_of("DiagnosticHint") end,
}
for i = 1, 8 do
  if not ansi[i] then
    fallback = true
    ansi[i] = (i == 1 and bg) or (i == 8 and fg) or hues[i]()
  end
end
for i = 9, 16 do
  if not ansi[i] then
    fallback = true
    ansi[i] = ansi[i - 8]
  end
end
for i = 1, 16 do
  assert(ansi[i], vim.env.CS .. ": no colour for ANSI " .. (i - 1))
end

io.stdout:write(vim.json.encode({ bg = bg, fg = fg, ansi = ansi, mode = vim.o.background, fallback = fallback }))
