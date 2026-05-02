local M = {}

local function write_result(result_path, payload)
  vim.fn.writefile({ vim.json.encode(payload) }, result_path)
end

local function start_client(server, workspace)
  local client_id = vim.lsp.start({
    name = "fluent-lsp",
    cmd = { server },
    root_dir = workspace,
  })
  assert(client_id, "failed to start fluent-lsp")
  return client_id
end

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local server = assert(vim.env.FLUENT_LSP_BIN ~= "" and vim.env.FLUENT_LSP_BIN)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)
  local shown_message = nil

  vim.lsp.handlers["window/showMessage"] = function(_, params, _)
    shown_message = params.message
  end

  vim.cmd.edit(workspace .. "/locales/es/app.ftl")
  local client_id = start_client(server, workspace)

  local attached = vim.wait(5000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.id == client_id and client.server_capabilities.codeLensProvider then
        return true
      end
    end
    return false
  end, 50)
  assert(attached, "codelens provider not ready")

  vim.lsp.codelens.enable(true, { bufnr = 0 })
  local ready = vim.wait(5000, function()
    return #vim.lsp.codelens.get({ bufnr = 0, client_id = client_id }) > 0
  end, 50)
  assert(ready, "codelens did not populate")

  local lenses = vim.lsp.codelens.get({ bufnr = 0, client_id = client_id })
  assert(#lenses == 1, "expected exactly one codelens")
  assert(lenses[1].lens.command.title == "Show all 4 selector combinations", "unexpected codelens title")

  local found = vim.fn.searchpos("install-hint", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  vim.lsp.codelens.run({ client_id = client_id })

  local shown = vim.wait(5000, function()
    return shown_message ~= nil and shown_message:match("Press Ctrl %+ V") ~= nil
  end, 50)
  assert(shown, "codelens did not show full selector combinations")

  write_result(result_path, {
    ok = true,
    feature = "codelens_selector_combinations",
  })
  vim.cmd.qa()
end

return M
