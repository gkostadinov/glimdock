#pragma once
#include <atomic>
#include <stdint.h>

namespace flipper {
// One transport worker submits commands; only the NimBLE host executes them.
// NimBLE 1.x owns advertised devices until its GAP callback has returned. Its
// result vector must never be cleared from the worker or inside onResult().
class ScanMailbox {
public:
    enum class Action { Start, Stop };

    template<class Post>
    bool submit(Action action, uint32_t seconds, Post post) {
        bool expected = false;
        if(!pending_.compare_exchange_strong(expected, true)) return false;
        action_ = action;
        seconds_ = seconds;
        result_ = false;
        post();
        return true;
    }

    // Called by the host event queue, after any advertisement/completion
    // callback has unwound. start() also clears old results on this same task.
    template<class Scanner, class Complete>
    void execute(Scanner& scanner, Complete complete) {
        if(action_ == Action::Start) {
            if(scanner.isScanning()) scanner.stop();
            result_ = scanner.start(seconds_, complete, false);
        } else {
            result_ = scanner.stop();
        }
        pending_.store(false, std::memory_order_release);
    }

    bool pending() const { return pending_.load(std::memory_order_acquire); }
    // Read only after pending() is false, by the single submitting worker.
    bool result() const { return result_; }

private:
    std::atomic<bool> pending_{false};
    Action action_ = Action::Stop;
    uint32_t seconds_ = 0;
    bool result_ = false;
};
}
