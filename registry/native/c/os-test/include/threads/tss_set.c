#include <threads.h>
#ifdef tss_set
#undef tss_set
#endif
int (*foo)(tss_t, void *) = tss_set;
int main(void) { return 0; }
