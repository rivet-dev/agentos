#include <sys/socket.h>
#ifdef recvmsg
#undef recvmsg
#endif
ssize_t (*foo)(int, struct msghdr *, int) = recvmsg;
int main(void) { return 0; }
