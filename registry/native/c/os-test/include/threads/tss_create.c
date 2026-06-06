#include <threads.h>
#ifdef tss_create
#undef tss_create
#endif
int (*foo)(tss_t *, tss_dtor_t) = tss_create;
int main(void) { return 0; }
