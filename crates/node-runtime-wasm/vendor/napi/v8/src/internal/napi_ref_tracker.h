#ifndef NAPI_V8_REF_TRACKER_H_
#define NAPI_V8_REF_TRACKER_H_

struct napi_ref_tracker__ {
  using RefList = napi_ref_tracker__;

  napi_ref_tracker__() = default;
  virtual ~napi_ref_tracker__() = default;

  void Link(RefList* list);
  void Unlink();
  virtual void Finalize() {}

  static void FinalizeAll(RefList* list);

 private:
  napi_ref_tracker__* next_ = nullptr;
  napi_ref_tracker__* prev_ = nullptr;
};

#endif  // NAPI_V8_REF_TRACKER_H_
