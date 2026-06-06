#include <unistd.h>
#ifdef getppid
#undef getppid
#endif
pid_t (*foo)(void) = getppid;
int main(void) { return 0; }
