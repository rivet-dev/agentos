#include <sys/socket.h>
#ifdef socket
#undef socket
#endif
int (*foo)(int, int, int) = socket;
int main(void) { return 0; }
