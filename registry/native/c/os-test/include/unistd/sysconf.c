#include <unistd.h>
#ifdef sysconf
#undef sysconf
#endif
long (*foo)(int) = sysconf;
int main(void) { return 0; }
