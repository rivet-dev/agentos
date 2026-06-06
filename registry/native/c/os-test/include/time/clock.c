#include <time.h>
#ifdef clock
#undef clock
#endif
clock_t (*foo)(void) = clock;
int main(void) { return 0; }
