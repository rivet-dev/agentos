#include <poll.h>
#ifdef poll
#undef poll
#endif
int (*foo)(struct pollfd [], nfds_t, int) = poll;
int main(void) { return 0; }
