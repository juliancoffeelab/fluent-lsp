local M = {}

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local server = assert(vim.env.FLUENT_LSP_BIN ~= "" and vim.env.FLUENT_LSP_BIN)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)

  vim.cmd.edit(workspace .. "/src/app.ts")

  local client_id = vim.lsp.start({
    name = "fluent-lsp",
    cmd = { server },
    root_dir = workspace,
  })
  assert(client_id, "failed to start fluent-lsp")

  local found = vim.fn.searchpos("welcome-title", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })

  local attached = vim.wait(5000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.name == "fluent-lsp" and client.server_capabilities.definitionProvider then
        return true
      end
    end
    return false
  end, 50)
  assert(attached, "LSP client did not initialize")

  vim.lsp.buf.definition()

  local jumped = vim.wait(5000, function()
    return vim.api.nvim_buf_get_name(0):match("locales/en/app%.ftl$") ~= nil
  end, 50)
  assert(jumped, "definition did not jump to app.ftl")

  local payload = vim.json.encode({
    file = vim.api.nvim_buf_get_name(0),
    line = vim.api.nvim_win_get_cursor(0)[1],
    col = vim.api.nvim_win_get_cursor(0)[2],
  })
  vim.fn.writefile({ payload }, result_path)
  vim.cmd.qa()
end

return M
