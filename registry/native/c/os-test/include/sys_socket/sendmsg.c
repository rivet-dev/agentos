#include <sys/socket.h>
#ifdef sendmsg
#undef sendmsg
#endif
ssize_t (*foo)(int, const struct msghdr *, int) = sendmsg;
int main(void) { return 0; }
