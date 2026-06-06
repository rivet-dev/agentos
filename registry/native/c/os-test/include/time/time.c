#include <time.h>
#ifdef time
#undef time
#endif
time_t (*foo)(time_t *) = time;
int main(void) { return 0; }
