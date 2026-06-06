#include <threads.h>
#ifdef tss_get
#undef tss_get
#endif
void *(*foo)(tss_t) = tss_get;
int main(void) { return 0; }
