math.randomseed(os.time())

local function b64(data)
    local b = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
    local res = ((data:gsub('.', function(x)
        local r, b = '', x:byte()
        for i = 8, 1, -1 do r = r .. (b % 2 ^ i - b % 2 ^ (i - 1) > 0 and '1' or '0') end
        return r;
    end) .. '0000'):gsub('%d%d%d?%d?%d?%d?', function(x)
        if (#x < 6) then return '' end
        local c = 0
        for i = 1, 6 do c = c + (x:sub(i, i) == '1' and 2 ^ (6 - i) or 0) end
        return b:sub(c + 1, c + 1)
    end))
    res = res:gsub('%+', '-'):gsub('/', '_'):gsub('=', '')
    return res
end

local tokens = {}
local token_count = 9000

for i = 1, token_count do
    local id = tostring(1000 + i)
    local payload = '{"sub":"' .. id .. '"}'
    tokens[i] = "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9." .. b64(payload) .. "."
end

wrk.method = "POST"

request = function()
    local token = tokens[math.random(1, token_count)]

    wrk.headers["authorization"] = token
    wrk.headers["x-forwarded-host"] = "that-limit.com"
    return wrk.format()
end
