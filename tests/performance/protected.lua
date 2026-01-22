math.randomseed(os.time())

wrk.method = "POST"

request = function()
    local id = tostring(math.random(1000, 9999))
    wrk.headers["user_id"] = id
    return wrk.format()
end
