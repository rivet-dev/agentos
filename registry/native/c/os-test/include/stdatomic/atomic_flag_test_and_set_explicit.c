#include <stdatomic.h>
#ifdef atomic_flag_test_and_set_explicit
#undef atomic_flag_test_and_set_explicit
#endif
_Bool (*foo)( volatile atomic_flag *, memory_order) = atomic_flag_test_and_set_explicit;
int main(void) { return 0; }
