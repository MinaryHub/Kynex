// Starts a DRL program on a Doosan Robotics controller — the emulator — and
// holds the connection while it runs: the controller stops a program whose
// client goes. What the program does shows in the emulator's log (see
// check.py): its tp_log lines, its end, any exception.
//
// The program's tool point, if it names one in a line
//   # TCP "KYNEX" ... posx(x, y, z, a, b, c)
// is registered first, as a teach pendant would.
//
// Built with Doosan's DRFL (see Dockerfile). Usage:
//   run_drl <host> <program.drl> [seconds to hold, 3600 by default]

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <regex>
#include <sstream>
#include <string>
#include <thread>

#include "DRFLEx.h"

using namespace DRAFramework;

static CDRFLEx robot;

static void sleep_ms(int ms) { std::this_thread::sleep_for(std::chrono::milliseconds(ms)); }

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "usage: run_drl <host> <program.drl> [seconds]\n");
        return 2;
    }
    std::ifstream file(argv[2]);
    std::stringstream program;
    program << file.rdbuf();
    const std::string text = program.str();
    const int hold = argc > 3 ? std::atoi(argv[3]) : 3600;

    if (!robot.open_connection(argv[1], 12345)) {
        std::fprintf(stderr, "no controller at %s:12345\n", argv[1]);
        return 1;
    }
    robot.setup_monitoring_version(1);
    robot.manage_access_control(MANAGE_ACCESS_CONTROL_FORCE_REQUEST);
    sleep_ms(500);
    // Servo on, out of any stop it was left in.
    for (int i = 0; i < 100 && robot.get_robot_state() != STATE_STANDBY; i++) {
        robot.set_robot_control(CONTROL_SERVO_ON);
        sleep_ms(100);
    }
    if (robot.get_robot_state() != STATE_STANDBY) {
        std::fprintf(stderr, "the robot is not standing by: state %d\n", (int)robot.get_robot_state());
        return 1;
    }

    std::smatch tcp;
    static const std::regex named(R"re(# TCP "([A-Za-z0-9_]+)"[^\n]*posx\(([^)]*)\))re");
    if (std::regex_search(text, tcp, named)) {
        float at[NUM_TASK] = {0};
        std::stringstream numbers(tcp[2].str());
        std::string n;
        for (int i = 0; i < NUM_TASK && std::getline(numbers, n, ','); i++) at[i] = std::stof(n);
        robot.del_tcp(tcp[1].str());
        bool added = robot.add_tcp(tcp[1].str(), at);
        std::printf("tcp %s %s\n", tcp[1].str().c_str(), added ? "registered" : "NOT registered");
    }

    robot.set_robot_mode(ROBOT_MODE_AUTONOMOUS);
    robot.set_robot_system(ROBOT_SYSTEM_VIRTUAL);
    sleep_ms(500);
    std::printf("started\n");
    std::fflush(stdout);
    // drl_start does not answer while the program runs, and nothing else
    // does meanwhile: it runs aside, and this holds on.
    std::thread([text] { robot.drl_start(ROBOT_SYSTEM_VIRTUAL, text); }).detach();
    std::this_thread::sleep_for(std::chrono::seconds(hold));
    std::_Exit(0);
}
