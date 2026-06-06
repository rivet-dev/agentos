#include <unistd.h>
#ifdef pause
#undef pause
#endif
int (*foo)(void) = pause;
int main(void) { return 0; }
