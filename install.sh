cargo build --release
sudo mv beat.service /etc/systemd/system/beat.service
sudo systemctl daemon-reload
sudo systemctl enable beat
sudo systemctl restart beat
