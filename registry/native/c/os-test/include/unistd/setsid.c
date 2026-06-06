#include <unistd.h>
#ifdef setsid
#undef setsid
#endif
pid_t (*foo)(void) = setsid;
int main(void) { return 0; }
