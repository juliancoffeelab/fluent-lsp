local M = {}

local traces = {}

local function write_result(path, payload)
  vim.fn.writefile({ vim.json.encode(payload) }, path)
end

local function start_client(server, workspace)
  local client_id = vim.lsp.start({
    name = "fluent-lsp",
    cmd = { server },
    root_dir = workspace,
    handlers = {
      ["$/logTrace"] = function(_, result)
        table.insert(traces, result)
      end,
    },
  })
  assert(client_id, "failed to start fluent-lsp")
  return client_id
end

local function wait_client()
  local attached = vim.wait(3000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.name == "fluent-lsp" and client.server_capabilities.definitionProvider then
        return true
      end
    end
    return false
  end, 50)
  assert(attached, "fluent-lsp did not attach")
end

local function request(method, params)
  local result = vim.lsp.buf_request_sync(0, method, params, 3000)
  assert(result, method .. " timed out")
  for _, response in pairs(result) do
    assert(not response.err, method .. " failed: " .. vim.inspect(response.err))
    return response.result
  end
  error(method .. " returned no client response")
end

local function position_of(needle)
  local found = vim.fn.searchpos(needle, "n")
  assert(found[1] > 0, "missing " .. needle)
  return { line = found[1] - 1, character = found[2] - 1 }
end

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local server = assert(vim.env.FLUENT_LSP_BIN ~= "" and vim.env.FLUENT_LSP_BIN)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)

  vim.cmd.edit(workspace .. "/locales/es/app.ftl")
  local client_id = start_client(server, workspace)
  wait_client()

  local client = vim.lsp.get_client_by_id(client_id)
  assert(client, "missing fluent-lsp client")
  client:notify("$/setTrace", { value = "verbose" })

  local definition = request("textDocument/definition", {
    textDocument = { uri = vim.uri_from_bufnr(0) },
    position = position_of("welcome-title"),
  })
  assert(definition.uri:match("locales/en/app%.ftl$"), "definition did not resolve to origin file")

  local saw_definition_timing = vim.wait(3000, function()
    for _, trace in ipairs(traces) do
      if trace.message and trace.message:find("operation=textDocument/definition", 1, true) then
        return true
      end
    end
    return false
  end, 50)
  assert(saw_definition_timing, "missing definition logTrace timing")

  local definition_trace
  for _, trace in ipairs(traces) do
    if trace.message and trace.message:find("operation=textDocument/definition", 1, true) then
      definition_trace = trace
      break
    end
  end

  write_result(result_path, {
    ok = true,
    feature = "log_trace_timings",
    message = definition_trace.message,
    verbose = definition_trace.verbose,
  })
  vim.cmd.qa()
end

return M
