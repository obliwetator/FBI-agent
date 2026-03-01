#! /bin/bash

cargo build --release && sudo systemctl restart fbi-agent.service 
