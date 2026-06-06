#include <time.h>
#ifdef timespec_get
#undef timespec_get
#endif
int (*foo)(struct timespec *, int) = timespec_get;
int main(void) { return 0; }
