local M = {}

local function write_result(result_path, payload)
  vim.fn.writefile({ vim.json.encode(payload) }, result_path)
end

local function current_buffer_text()
  return table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n") .. "\n"
end

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)
  local app_path = workspace .. "/locales/es/app.ftl"
  local menu_path = workspace .. "/locales/es/dialogs/menu.ftl"

  vim.cmd.edit(app_path)
  vim.api.nvim_buf_set_lines(0, 0, -1, false, {
    "welcome-title = Bienvenido",
    "",
    "down",
  })
  local top_level_source = current_buffer_text()

  vim.cmd.edit(menu_path)
  vim.api.nvim_buf_set_lines(0, 0, -1, false, {
    "menu-save =",
    "    .",
  })
  local bare_dot_source = current_buffer_text()

  vim.api.nvim_buf_set_lines(0, 0, -1, false, {
    "menu-save =",
    "    .l",
  })
  local attribute_prefix_source = current_buffer_text()

  write_result(result_path, {
    ok = true,
    feature = "completion_origin_keys",
    top_level_source = top_level_source,
    bare_dot_source = bare_dot_source,
    attribute_prefix_source = attribute_prefix_source,
  })
  vim.cmd("qa!")
end

return M
