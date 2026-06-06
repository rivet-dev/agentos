#include <sys/socket.h>
#ifdef recv
#undef recv
#endif
ssize_t (*foo)(int, void *, size_t, int) = recv;
int main(void) { return 0; }
