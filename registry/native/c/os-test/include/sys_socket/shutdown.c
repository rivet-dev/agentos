#include <sys/socket.h>
#ifdef shutdown
#undef shutdown
#endif
int (*foo)(int, int) = shutdown;
int main(void) { return 0; }
