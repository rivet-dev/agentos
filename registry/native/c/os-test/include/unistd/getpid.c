#include <unistd.h>
#ifdef getpid
#undef getpid
#endif
pid_t (*foo)(void) = getpid;
int main(void) { return 0; }
