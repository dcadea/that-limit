math.randomseed(os.time())

wrk.method = "POST"

request = function()
    local id = tostring(math.random(1000, 9999))
    wrk.headers["user_id"] = id
    wrk.headers["x-forwarded-host"] = "that-limit.com"
    return wrk.format()
end
