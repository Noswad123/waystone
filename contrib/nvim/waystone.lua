local M = {}

M.config = {
  command = "waystone",
  width = 0.9,
  height = 0.85,
}

function M.setup(opts)
  M.config = vim.tbl_extend("force", M.config, opts or {})

  vim.api.nvim_create_user_command("Waystone", function()
    M.pick()
  end, {})

  vim.api.nvim_create_user_command("WaystoneAddCurrent", function(command_opts)
    M.add_current(command_opts.args)
  end, { nargs = "?" })
end

local function close_window(win)
  if win and vim.api.nvim_win_is_valid(win) then
    vim.api.nvim_win_close(win, true)
  end
end

local function delete_buffer(buf)
  if buf and vim.api.nvim_buf_is_valid(buf) then
    vim.api.nvim_buf_delete(buf, { force = true })
  end
end

local function float_config()
  local columns = vim.o.columns
  local lines = vim.o.lines
  local width = math.floor(columns * M.config.width)
  local height = math.floor(lines * M.config.height)

  return {
    relative = "editor",
    width = width,
    height = height,
    col = math.floor((columns - width) / 2),
    row = math.floor((lines - height) / 2),
    style = "minimal",
    border = "rounded",
  }
end

local function read_first_line(path)
  local ok, lines = pcall(vim.fn.readfile, path)
  if not ok or not lines or not lines[1] then
    return nil
  end
  return lines[1]
end

local function parse_action(line)
  if not line or line == "" then
    return nil, nil
  end
  return line:match("^([^\t]+)\t(.*)$")
end

local function edit_path(path)
  vim.cmd.edit(vim.fn.fnameescape(path))
end

local function add_buffer(path)
  vim.cmd.badd(vim.fn.fnameescape(path))
end

local function handle_action(action, path)
  if not action or not path then
    return
  end

  if action == "open" or action == "edit" then
    edit_path(path)
  elseif action == "edit-return" then
    add_buffer(path)
    vim.schedule(M.pick)
  else
    vim.notify("waystone: unknown action: " .. action, vim.log.levels.WARN)
  end
end

function M.pick()
  local output = vim.fn.tempname()
  local buf = vim.api.nvim_create_buf(false, true)
  local win = vim.api.nvim_open_win(buf, true, float_config())
  local shell_command = string.format(
    "%s select --action > %s",
    vim.fn.shellescape(M.config.command),
    vim.fn.shellescape(output)
  )

  vim.fn.termopen({ vim.o.shell, vim.o.shellcmdflag, shell_command }, {
    on_exit = function(_, code)
      vim.schedule(function()
        close_window(win)
        delete_buffer(buf)

        local line = read_first_line(output)
        vim.fn.delete(output)

        if code ~= 0 then
          vim.notify("waystone exited with status " .. code, vim.log.levels.ERROR)
          return
        end

        local action, path = parse_action(line)
        handle_action(action, path)
      end)
    end,
  })

  vim.cmd.startinsert()
end

function M.add_current(label)
  local path = vim.api.nvim_buf_get_name(0)
  if path == "" then
    vim.notify("waystone: current buffer has no file", vim.log.levels.WARN)
    return
  end

  local command = { M.config.command, "add", path }
  if label and label ~= "" then
    table.insert(command, label)
  end

  vim.fn.system(command)
  if vim.v.shell_error ~= 0 then
    vim.notify("waystone add failed", vim.log.levels.ERROR)
  else
    vim.notify("waystone: added " .. path)
  end
end

return M
