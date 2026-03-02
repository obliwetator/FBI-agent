#! /bin/bash

cargo build && sudo systemctl restart fbi-agent-debug.service 
