vim.opt.swapfile = false
vim.opt.undofile = false
vim.opt.shadafile = "NONE"

local config_dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h")
vim.opt.runtimepath:prepend(config_dir)
