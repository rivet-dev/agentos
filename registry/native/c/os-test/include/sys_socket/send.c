#include <sys/socket.h>
#ifdef send
#undef send
#endif
ssize_t (*foo)(int, const void *, size_t, int) = send;
int main(void) { return 0; }
