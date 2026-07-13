export interface ActorSecretDraftState {
  addOpen: boolean;
  addKey: string;
  addValue: string;
  addValueTouched: boolean;
  showAddValue: boolean;
  editingKey: string;
  editValue: string;
  editValueTouched: boolean;
  showEditValue: boolean;
}

export type ActorSecretDraftAction =
  | { type: "openAdd" }
  | { type: "setAddKey"; value: string }
  | { type: "setAddValue"; value: string }
  | { type: "toggleAddVisibility" }
  | { type: "closeAdd" }
  | { type: "startEdit"; key: string }
  | { type: "setEditValue"; value: string }
  | { type: "toggleEditVisibility" }
  | { type: "closeEdit" }
  | { type: "discardKey"; key: string }
  | { type: "discardAll" };

export function emptyActorSecretDraftState(): ActorSecretDraftState {
  return {
    addOpen: false,
    addKey: "",
    addValue: "",
    addValueTouched: false,
    showAddValue: false,
    editingKey: "",
    editValue: "",
    editValueTouched: false,
    showEditValue: false,
  };
}

function closeAdd(state: ActorSecretDraftState): ActorSecretDraftState {
  return {
    ...state,
    addOpen: false,
    addKey: "",
    addValue: "",
    addValueTouched: false,
    showAddValue: false,
  };
}

function closeEdit(state: ActorSecretDraftState): ActorSecretDraftState {
  return { ...state, editingKey: "", editValue: "", editValueTouched: false, showEditValue: false };
}

export function actorSecretDraftReducer(
  state: ActorSecretDraftState,
  action: ActorSecretDraftAction,
): ActorSecretDraftState {
  switch (action.type) {
    case "openAdd":
      return { ...state, addOpen: true };
    case "setAddKey":
      return { ...state, addKey: action.value };
    case "setAddValue":
      return { ...state, addValue: action.value, addValueTouched: true };
    case "toggleAddVisibility":
      return { ...state, showAddValue: !state.showAddValue };
    case "closeAdd":
      return closeAdd(state);
    case "startEdit":
      return { ...closeEdit(state), editingKey: action.key };
    case "setEditValue":
      return { ...state, editValue: action.value, editValueTouched: true };
    case "toggleEditVisibility":
      return { ...state, showEditValue: !state.showEditValue };
    case "closeEdit":
      return closeEdit(state);
    case "discardKey": {
      const withoutAdd = state.addKey.trim() === action.key ? closeAdd(state) : state;
      return withoutAdd.editingKey === action.key ? closeEdit(withoutAdd) : withoutAdd;
    }
    case "discardAll":
      return emptyActorSecretDraftState();
  }
}
