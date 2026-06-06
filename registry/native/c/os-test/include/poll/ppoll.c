#include <poll.h>
#ifdef ppoll
#undef ppoll
#endif
int (*foo)(struct pollfd [], nfds_t, const struct timespec *restrict, const sigset_t *restrict) = ppoll;
int main(void) { return 0; }
